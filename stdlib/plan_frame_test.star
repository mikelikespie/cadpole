# The tests of @cadpole/plan_frame.star: reframe() on synthetic plans -- a box on its centre wall, a room across
# the line (a beam), a door in a bearing wall (a header), stair openings on and off the lines (R502.10), upper walls
# off the line below (R502.4: a transfer past the joist depth, none within it), concrete walls, the direction, and
# a benchmark plan (a synthetic 25 x 100-ft lot), which reframe()
# frames in well under a second. `cadpole test stdlib/plan_frame_test.star`.

load("@cadpole/plan_frame.star", "frame_checks", "frame_members", "frame_menu", "frame_set", "frame_text", "reframe")
load("@cadpole/plan_spans.star", "ftin")

def room(key, floor, kind, rect, **kw):
    r = {"key": key, "name": key, "floor": floor, "kind": kind, "rects": [rect]}
    r.update(kw)
    return r

def flr(key, z, plate, ceiling=8.5):
    return {"key": key, "z": z, "ceiling": ceiling, "plate": [plate]}

def plan(name, floors, rooms, doors=None, windows=None, walls=None):
    return {"name": name, "unit": "ft", "floors": floors, "rooms": rooms, "doors": doors or [], "windows": windows or [],
            "walls": walls or [], "bearing": [], "meta": {}}

def box(width=24.0, depth=40.0, wall=True):
    """Two floors, width x depth inside; a centre wall on both (or only the ground floor's back half: wall=False)."""
    x1, y1, c = 0.5 + width, 0.5 + depth, 0.5 + width / 2.0
    rooms = [
        room("liv", "G", "living", (0.5, c - 0.2, 0.5, 20.0)) if wall else room("liv", "G", "living", (0.5, x1, 0.5, 20.0)),
        room("din", "G", "dining", (c + 0.2, x1, 0.5, 20.0)) if wall else None,
        room("kit", "G", "kitchen", (0.5, c - 0.2, 20.4, y1)),
        room("fam", "G", "family", (c + 0.2, x1, 20.4, y1)),
        room("b1", "U", "bed", (0.5, c - 0.2, 0.5, 20.0)),
        room("b2", "U", "bed", (c + 0.2, x1, 0.5, 20.0)),
        room("ba", "U", "bath", (0.5, c - 0.2, 20.4, y1)),
        room("b3", "U", "bed", (c + 0.2, x1, 20.4, y1)),
    ]
    return plan("box", [flr("G", 0.0, (0.5, x1, 0.5, y1), 9.0), flr("U", 10.5, (0.5, x1, 0.5, y1))], [r for r in rooms if r != None])

def lines_of(p, floor):
    return sorted([b["at"] for b in p["bearing"] if b["floor"] == floor and b["kind"] != "exterior"])

def every_bay_holds(p):
    for j in frame_members(p, "joists"):
        assert_true(j["span"] <= j["max_span"] + 1e-9, msg="%s: %s clear, %s allowed" % (j["name"], ftin(j["span"]), ftin(j["max_span"])))
    return len(frame_members(p, "joists"))

def a_box_frames_on_its_centre_wall():
    p = reframe(box())
    fr = p["meta"]["plan_frame"]
    assert_eq(fr["status"], "optimal")
    lv = {x["floor"]: x for x in fr["levels"]}
    assert_eq(lv["G"]["direction"], "u")                 # across the 24 ft, not along the 40
    assert_eq(lv["G"]["carries"], "U")
    assert_eq(lv["U"]["carries"], "roof")
    assert_eq(lines_of(p, "G"), [12.5])                  # the centre wall, under the floor
    assert_eq(lines_of(p, "U"), [12.5])                  # and under the roof, stacked on it
    assert_eq(fr["counts"]["beams"], 0)
    assert_eq(fr["counts"]["transfers"], 0)
    every_bay_holds(p)
    # the plan comes back with its walls: the centre wall bearing on both floors, the exterior walls marked exterior
    brg = [w for w in p["walls"] if w["bearing"] and w["kind"] == "bearing"]
    assert_eq(sorted({w["floor"]: 1 for w in brg}.keys()), ["G", "U"])
    assert_true(len([w for w in p["walls"] if w["kind"] == "exterior"]) >= 4)
    return frame_text(p).split("\n")[0]

test("a box frames on its centre wall, floor and roof alike", a_box_frames_on_its_centre_wall)

def a_room_across_the_line_takes_a_beam():
    # 30 ft wide: nothing in the menu spans it, so a line is needed; the only wall is the back half's, and the front
    # room straddles it, so the line takes a beam over that room.
    p = reframe(box(width=30.0, wall=False))
    beams = frame_members(p, "beam", floor="G")
    assert_eq(len(beams), 1)
    b = beams[0]
    assert_eq(b["over"], "liv")
    assert_close(b["span"], 19.5, tol=1e-6)
    assert_true(b["plf"] > 0)
    assert_true(b["member"] != None)
    every_bay_holds(p)
    return b["text"]

test("a room across the bearing line takes a beam, sized", a_room_across_the_line_takes_a_beam)

def a_door_in_a_bearing_wall_takes_a_header():
    p0 = box()
    p0["doors"] = [{"key": "d1", "floor": "G", "a": "liv", "b": "din", "wall": "E", "at": (12.3, 10.0), "width": 3.0}]
    p = reframe(p0)
    hs = frame_members(p, "header")
    assert_eq(len(hs), 1)
    assert_eq(hs[0]["opening"], "d1")
    assert_eq(hs[0]["wall"], "interior")
    assert_true(hs[0]["rule"] == "R602.7(2)", msg=hs[0]["rule"])
    return hs[0]["text"]

test("a door in a bearing wall takes an R602.7 header", a_door_in_a_bearing_wall_takes_a_header)

def stair(rect, key, floor):
    return room(key, floor, "stair", rect, core="st")

def stair_plan(width, stair_rect, others):
    x1 = 0.5 + width
    rooms = []
    for f in ["G", "U"]:
        rooms.append(stair(stair_rect, "s_" + f, f))
        for (k, kind, r) in others:
            rooms.append(room(k + "_" + f, f, kind, r))
    return plan("stair", [flr("G", 0.0, (0.5, x1, 0.5, 30.5), 9.0), flr("U", 10.5, (0.5, x1, 0.5, 30.5))], rooms)

def stair_openings_take_headers_off_the_lines():
    # 12 ft wide, the stair in the middle of a band between two cross walls. Left free, the joists run along v and
    # bear on those two walls, so neither side of the opening across the joists needs a header.
    others = [("s", "living", (0.5, 12.5, 0.5, 10.0)), ("w", "hall", (0.5, 4.1, 10.4, 20.0)),
              ("e", "closet", (8.9, 12.5, 10.4, 20.0)), ("n", "dining", (0.5, 12.5, 20.4, 30.5))]
    free = reframe(stair_plan(12.0, (4.5, 8.5, 10.4, 20.0), others))
    assert_eq(free["meta"]["plan_frame"]["levels"][0]["direction"], "v")
    assert_eq(len(frame_members(free, "stair_header")), 0)
    # Joists across the 12 ft instead: the stair cuts them, and both of its sides across the joists take headers,
    # doubled past 4 ft (the opening runs 9'-7").
    mid = reframe(stair_plan(12.0, (4.5, 8.5, 10.4, 20.0), others), menu={"span": "u"})
    sh = frame_members(mid, "stair_header", floor="G")
    assert_eq(sorted([h["side"] for h in sh]), ["hi", "lo"])
    for h in sh:
        assert_close(h["span"], 9.6, tol=1e-6)
        assert_true(h["rule"].startswith("R502.10"), msg=h["rule"])
        assert_true("doubled" in h["size"], msg=h["size"])
    assert_eq(len(frame_members(mid, "stair_header", floor="U")), 0)   # nothing rises past the upper floor
    # 24 ft wide, the stair against the east wall: the one interior wall, beside the stair, is the line the joists
    # need, so the stair's west side is on it and its east side on the exterior wall: no header (R502.10).
    side = reframe(stair_plan(24.0, (20.5, 24.5, 10.4, 20.0), [
        ("w", "living", (0.5, 20.1, 0.5, 30.5)), ("cs", "closet", (20.5, 24.5, 0.5, 10.0)),
        ("cn", "closet", (20.5, 24.5, 20.4, 30.5))]))
    assert_eq(lines_of(side, "G"), [20.3])
    assert_eq(len(frame_members(side, "stair_header")), 0)
    return [h["text"] for h in sh]

test("a stair opening takes R502.10 headers on the sides off the lines, none on a line", stair_openings_take_headers_off_the_lines)

def two_storey_row(upper_wall):
    # 28 ft wide, sawn lumber only, so the roof needs a line (2x12 at 12 in spans 26'-0" under a roof): the ground
    # floor's only wall is at u 12.2, the upper floor's at `upper_wall`.
    x1 = 28.5
    return plan("row", [flr("G", 0.0, (0.5, x1, 0.5, 30.5), 9.0), flr("U", 10.5, (0.5, x1, 0.5, 30.5))], [
        room("w", "G", "living", (0.5, 12.0, 0.5, 30.5)), room("e", "G", "dining", (12.4, x1, 0.5, 30.5)),
        room("uw", "U", "bed", (0.5, upper_wall - 0.2, 0.5, 30.5)), room("ue", "U", "bed", (upper_wall + 0.2, x1, 0.5, 30.5))])

def an_upper_wall_off_the_line_is_a_transfer():
    sawn = {"ijoists": [], "span": "u"}
    far = reframe(two_storey_row(16.0), menu=sawn)
    ts = frame_members(far, "transfer")
    assert_eq(len(ts), 1)
    t = ts[0]
    assert_eq(t["floor"], "G")          # it is in the level over G, under U's wall
    assert_eq(t["name"], "G T1")        # and named by that level
    assert_eq(t["under"], "U")
    assert_close(t["at"], 16.0, tol=1e-6)
    assert_close(t["offset"], 3.8, tol=1e-6)
    rows = [r for r in frame_checks(far) if r["rule"] == "R502.4"]
    assert_eq(len(rows), 1)
    assert_eq(rows[0]["severity"], "warn")
    # 8 3/8 in off the line below, within its joists' depth: R502.4 lets it stand there
    near = reframe(two_storey_row(12.9), menu=sawn)
    assert_eq(len(frame_members(near, "transfer")), 0)
    lv = {x["floor"]: x for x in near["meta"]["plan_frame"]["levels"]}
    assert_true(lv["G"]["depth"] / 12.0 >= 0.7)
    return t["text"]

test("an upper bearing wall off the line below is a transfer past the joist depth, not within it (R502.4)", an_upper_wall_off_the_line_is_a_transfer)

def concrete_walls_are_founded_supports():
    # a lower floor in concrete under the west half; the ground floor over it and a garage on grade to the east; the
    # wall between them stands on the concrete wall; a door in the concrete wall takes a lintel.
    walls = [{"floor": "B", "a": (20.75, 0.25), "b": (20.75, 40.75), "t": 0.5, "kind": "concrete"},
             {"floor": "B", "a": (10.5, 0.5), "b": (10.5, 40.5), "t": 0.5, "kind": "concrete"}]
    p0 = plan("cellar", [flr("B", -10.0, (0.5, 20.5, 0.5, 40.5), 9.0), flr("G", 0.0, (0.5, 40.5, 0.5, 40.5), 9.0),
                         flr("U", 10.5, (0.5, 40.5, 0.5, 40.5))], [
        room("bw", "B", "play", (0.5, 10.25, 0.5, 40.5)), room("be", "B", "storage", (10.75, 20.5, 0.5, 40.5)),
        room("liv", "G", "living", (0.5, 20.5, 0.5, 40.5)), room("gar", "G", "garage", (21.0, 40.5, 0.5, 40.5)),
        room("uw", "U", "bed", (0.5, 20.5, 0.5, 40.5)), room("ue", "U", "bed", (21.0, 40.5, 0.5, 40.5))],
        doors=[{"key": "d_b", "floor": "B", "a": "bw", "b": "be", "wall": "E", "at": (10.5, 20.0), "width": 3.0}],
        walls=walls)
    p = reframe(p0)
    fr = p["meta"]["plan_frame"]
    assert_eq(fr["counts"]["transfers"], 0)
    kinds = {b["kind"]: True for b in p["bearing"] if b["floor"] == "B"}
    assert_true("concrete" in kinds, msg=str(p["bearing"]))
    lint = [h for h in frame_members(p, "header", floor="B") if h["opening"] == "d_b"]
    assert_eq(len(lint), 1)
    assert_eq(lint[0]["rule"], "concrete wall")
    # the ground floor's wall over the concrete wall is a line of the level over G (the garage's west wall)
    assert_true([b for b in p["bearing"] if b["floor"] == "G" and abs(b["at"] - 20.75) < 0.3], msg=str(lines_of(p, "G")))
    every_bay_holds(p)
    return lines_of(p, "B")

test("concrete walls are founded supports, and their openings take lintels", concrete_walls_are_founded_supports)

def the_direction_is_chosen_and_can_be_set():
    p = reframe(box(width=20.0, depth=50.0))
    lv = {x["floor"]: x for x in p["meta"]["plan_frame"]["levels"]}
    assert_eq(lv["G"]["direction"], "u")
    assert_eq(lv["G"]["lines_axis"], "v")
    q = reframe(box(width=20.0, depth=50.0), menu={"span": "v"})
    lq = {x["floor"]: x for x in q["meta"]["plan_frame"]["levels"]}
    assert_eq(lq["G"]["direction"], "v")
    assert_true(q["meta"]["plan_frame"]["score"] > p["meta"]["plan_frame"]["score"])
    return [p["meta"]["plan_frame"]["score"], q["meta"]["plan_frame"]["score"]]

test("joists run the short way unless the menu says otherwise", the_direction_is_chosen_and_can_be_set)

def the_menu_refuses_what_it_does_not_know():
    m = frame_menu(dead=10, costs={"line": 3.0})
    assert_eq(m["dead"], 10)
    assert_eq(m["costs"]["line"], 3.0)
    assert_eq(m["costs"]["wall"], 0.5)   # the other weights stay
    return m["costs"]["line"]

test("frame_menu takes overrides and keeps the rest", the_menu_refuses_what_it_does_not_know)

# A benchmark plan: a synthetic 25 x 100-ft lot, no real site.
def bench_plan(deck=True):
    """The benchmark plan:
    a synthetic 25 x 100-ft lot, three floors, party walls at u 0.5 and 24.5, the street at v 0."""
    rooms = [
        {"key": "garage", "name": "garage", "floor": "G", "kind": "garage", "rects": [(0.5, 14.312, 5.5, 25.5)]},
        {"key": "entry", "name": "entry", "floor": "G", "kind": "entry", "rects": [(20.688, 24.5, 5.5, 19.75)]},
        {"key": "hall_G", "name": "hall_G", "floor": "G", "kind": "hall", "rects": [(14.688, 20.312, 15.875, 48.125)]},
        {"key": "den", "name": "den", "floor": "G", "kind": "den", "rects": [(0.5, 24.5, 48.5, 64.5)]},
        {"key": "bath_G", "name": "bath_G", "floor": "G", "kind": "bath", "rects": [(14.688, 20.312, 5.5, 15.5)]},
        {"key": "storage", "name": "storage", "floor": "G", "kind": "storage", "rects": [(0.5, 14.312, 25.875, 48.125)]},
        {"key": "clG1", "name": "clG1", "floor": "G", "kind": "closet", "rects": [(20.688, 24.5, 34.5, 37.75)]},
        {"key": "clG2", "name": "clG2", "floor": "G", "kind": "closet", "rects": [(20.688, 24.5, 38.125, 48.125)]},
        {"key": "stair_G", "name": "stair_G", "floor": "G", "kind": "stair", "rects": [(20.688, 24.5, 20.125, 34.125)], "core": "st"},
        {"key": "clM1", "name": "clM1", "floor": "M", "kind": "closet", "rects": [(0.5, 8.312, 20.125, 27.193)]},
        {"key": "clM2", "name": "clM2", "floor": "M", "kind": "closet", "rects": [(0.5, 6.312, 27.568, 34.125)]},
        {"key": "living", "name": "living", "floor": "M", "kind": "living", "rects": [(0.5, 24.5, 5.5, 19.75)]},
        {"key": "dining", "name": "dining", "floor": "M", "kind": "dining", "rects": [(14.688, 24.5, 34.5, 50.5)]},
        {"key": "kitchen", "name": "kitchen", "floor": "M", "kind": "kitchen", "rects": [(0.5, 14.312, 34.5, 50.5)]},
        {"key": "powder", "name": "powder", "floor": "M", "kind": "powder", "rects": [(8.688, 14.312, 20.125, 27.193)]},
        {"key": "pantry", "name": "pantry", "floor": "M", "kind": "pantry", "rects": [(6.688, 14.312, 27.568, 34.125)]},
        {"key": "hall_M", "name": "hall_M", "floor": "M", "kind": "hall", "rects": [(14.688, 20.312, 20.125, 34.125)]},
        {"key": "stair_M", "name": "stair_M", "floor": "M", "kind": "stair", "rects": [(20.688, 24.5, 20.125, 34.125)], "core": "st"},
        {"key": "clU1", "name": "clU1", "floor": "U", "kind": "closet", "rects": [(16.688, 20.312, 16.892, 25.75)]},
        {"key": "clU2", "name": "clU2", "floor": "U", "kind": "closet", "rects": [(20.688, 24.5, 16.892, 19.75)]},
        {"key": "hall_U", "name": "hall_U", "floor": "U", "kind": "hall", "rects": [(16.688, 24.5, 34.5, 37.5)]},
        {"key": "hall_U2", "name": "hall_U2", "floor": "U", "kind": "hall", "rects": [(12.688, 16.312, 16.892, 37.5)]},
        {"key": "bed1", "name": "bed1", "floor": "U", "kind": "bed", "rects": [(0.5, 24.5, 37.875, 50.5)]},
        {"key": "bath1", "name": "bath1", "floor": "U", "kind": "bath", "rects": [(0.5, 12.312, 29.53, 37.5)]},
        {"key": "closet1", "name": "closet1", "floor": "U", "kind": "closet", "rects": [(0.5, 6.312, 20.267, 29.155)]},
        {"key": "bed2", "name": "bed2", "floor": "U", "kind": "bed", "rects": [(0.5, 12.312, 5.5, 19.892)]},
        {"key": "bed3", "name": "bed3", "floor": "U", "kind": "bed", "rects": [(12.688, 24.5, 5.5, 16.517)]},
        {"key": "bath2", "name": "bath2", "floor": "U", "kind": "bath", "rects": [(6.688, 12.312, 20.267, 29.155)]},
        {"key": "laundry", "name": "laundry", "floor": "U", "kind": "laundry", "rects": [(16.688, 20.312, 26.125, 34.125)]},
        {"key": "stair_U", "name": "stair_U", "floor": "U", "kind": "stair", "rects": [(20.688, 24.5, 20.125, 34.125)], "core": "st"},
    ]
    if deck:
        rooms.append({"key": "deck", "name": "deck", "floor": "M", "kind": "deck", "rects": [(0.5, 24.5, 50.875, 64.5)]})
    return {"name": "bench", "unit": "ft", "source": "a free-solved benchmark plan", "rooms": rooms, "doors": [], "windows": [],
            "floors": [{"key": "G", "z": 0.0, "ceiling": 8.5, "plate": [(0.5, 24.5, 5.5, 64.5)]},
                       {"key": "M", "z": 10.0, "ceiling": 8.5, "plate": [(0.5, 24.5, 5.5, 50.5)]},
                       {"key": "U", "z": 20.0, "ceiling": 8.5, "plate": [(0.5, 24.5, 5.5, 50.5)]}]}

def the_benchmark_plan_reframes():
    p = reframe(bench_plan())
    fr = p["meta"]["plan_frame"]
    assert_eq(fr["status"], "optimal")
    assert_eq(fr["solve"]["gap"], 0.0)
    assert_eq(fr["counts"]["transfers"], 0)
    assert_true(fr["counts"]["beams"] <= 4, msg=frame_text(p))
    assert_eq(len([b for b in frame_members(p, "beam") if b["room_kind"] == "garage"]), 0)   # nothing over the car
    lv = {x["floor"]: x for x in fr["levels"]}
    assert_eq(lv["U"]["carries"], "roof")
    assert_eq(len([b for b in p["bearing"] if b["floor"] == "U" and b["kind"] == "interior"]), 0)   # the roof spans wall to wall
    every_bay_holds(p)
    return fr["counts"]

test("a free-solved benchmark plan reframes: no transfer, nothing over the garage", the_benchmark_plan_reframes)

def framing_plans_draw():
    p = reframe(bench_plan(), name="plan_frame_bench_drawn")
    S = frame_set(p)
    assert_eq(S.sheets(), ["S1.1", "S1.2"])   # two levels side by side at 1/4" = 1'-0", then the roof's
    return S.sheets()

test("framing plans: the levels side by side on ARCH D sheets, with their schedules", framing_plans_draw)

def overhangs_are_flagged_and_grade_is_not():
    # the upper floor stands 3 ft past the ground floor's front wall: a cantilever beyond the joist depth (R502.3.3)
    p = plan("bay", [flr("G", 0.0, (0.5, 24.5, 3.5, 40.5), 9.0), flr("U", 10.5, (0.5, 24.5, 0.5, 40.5))], [
        room("a", "G", "living", (0.5, 12.0, 3.5, 40.5)), room("b", "G", "dining", (12.4, 24.5, 3.5, 40.5)),
        room("c", "U", "bed", (0.5, 12.0, 0.5, 40.5)), room("d", "U", "bed", (12.4, 24.5, 0.5, 40.5))])
    rows = [r for r in frame_checks(reframe(p)) if r["rule"] == "R502.3.3"]
    assert_eq(len(rows), 1)
    assert_eq(rows[0]["rect"], (0.5, 24.5, 0.5, 3.5))
    # a ground floor (z 0) running past a basement is on grade there, not an overhang
    q = plan("dig", [flr("B", -10.0, (0.5, 12.5, 0.5, 40.5), 9.0), flr("G", 0.0, (0.5, 24.5, 0.5, 40.5), 9.0)], [
        room("x", "B", "play", (0.5, 12.5, 0.5, 40.5)), room("a", "G", "living", (0.5, 12.0, 0.5, 40.5)),
        room("b", "G", "garage", (12.4, 24.5, 0.5, 40.5))])
    assert_eq([r for r in frame_checks(reframe(q)) if r["rule"] == "R502.3.3"], [])
    return rows[0]["detail"]

test("a floor standing past the one below its joist depth is flagged (R502.3.3); a floor on grade is not", overhangs_are_flagged_and_grade_is_not)

def slabs_leave_the_joist_model():
    # the garage's floor over a dug room as a structural slab: no joists there, one engineered slab member
    q = plan("slab", [flr("B", -10.0, (0.5, 24.5, 0.5, 40.5), 9.0), flr("G", 0.0, (0.5, 24.5, 0.5, 40.5), 9.0)], [
        room("x", "B", "play", (0.5, 12.0, 0.5, 40.5)), room("y", "B", "storage", (12.4, 24.5, 0.5, 40.5)),
        room("a", "G", "living", (0.5, 12.0, 0.5, 40.5)), room("b", "G", "garage", (12.4, 24.5, 0.5, 40.5))])
    p = reframe(q, menu={"slabs": [{"floor": "B", "rect": (12.4, 24.5, 0.5, 40.5)}]})
    sl = frame_members(p, "slab")
    assert_eq(len(sl), 1)
    assert_eq(sl[0]["floor"], "B")
    for j in frame_members(p, "joists", floor="B"):
        assert_true(j["rect"][1] <= 12.4 + 1e-6, msg=str(j["rect"]))
    return sl[0]["text"]

test("a structural slab in the menu leaves the joist model and is listed, engineered", slabs_leave_the_joist_model)

def separate_buildings_stack_on_their_own():
    # a two-floor house and, beside it, a kept older building a few feet lower: each floor's level carries the floor
    # that overlaps it, the older building (every room kept) is not reframed, and a court open over the ground floor is
    # not joisted
    p0 = plan("lot", [flr("G", 0.0, (0.5, 24.5, 0.5, 30.5), 9.0), flr("C", -3.0, (0.5, 20.5, 40.0, 60.0), 8.0),
                      flr("U", 10.5, (0.5, 24.5, 0.5, 30.5))], [
        room("a", "G", "living", (0.5, 12.0, 0.5, 30.5)), room("b", "G", "dining", (12.4, 24.5, 0.5, 30.5)),
        room("ca", "C", "living", (0.5, 20.5, 40.0, 60.0), kept=True),
        room("c", "U", "bed", (0.5, 12.0, 0.5, 30.5)), room("d", "U", "bed", (12.4, 24.5, 0.5, 30.5))])
    p0["courts"] = [{"key": "lw", "kind": "well", "rect": (16.0, 24.5, 12.0, 18.0), "base": "G", "top": 30.0, "reaches": None}]
    p = reframe(p0)
    fr = p["meta"]["plan_frame"]
    lv = {x["floor"]: x for x in fr["levels"]}
    assert_eq(sorted(lv.keys()), ["G", "U"])
    assert_eq(lv["G"]["carries"], "U")              # not the older building, though it sits between them in z
    assert_true([n for n in fr["notes"] if "Floor C" in n])
    for j in frame_members(p, "joists", floor="G"):
        r = j["rect"]
        assert_true(not (r[0] < 23.0 and r[1] > 17.0 and r[2] < 17.0 and r[3] > 13.0), msg="joists across the well: %s" % str(r))
    return fr["counts"]

test("separate buildings stack on their own, kept floors are not reframed, open courts are not joisted", separate_buildings_stack_on_their_own)

def the_frame_as_drawn_against_the_cheapest():
    # the plan says its upper floor bears on u 16.0 (a transfer over the ground floor's line at 12.2); framed as
    # drawn it costs more than the free frame, which finds the same.
    p0 = two_storey_row(16.0)
    p0["bearing"] = [{"floor": "G", "axis": "v", "at": 12.2, "span": (0.5, 30.5)}, {"floor": "U", "axis": "v", "at": 16.0, "span": (0.5, 30.5)}]
    drawn = reframe(p0, menu={"lines": "plan", "ijoists": []})
    free = reframe(p0, menu={"ijoists": []})
    assert_eq(drawn["meta"]["plan_frame"]["status"], "optimal")
    assert_eq(lines_of(drawn, "U"), [16.0])
    assert_eq(len(frame_members(drawn, "transfer")), 1)
    assert_true(drawn["meta"]["plan_frame"]["score"] >= free["meta"]["plan_frame"]["score"] - 1e-6)
    return [drawn["meta"]["plan_frame"]["score"], free["meta"]["plan_frame"]["score"]]

test("the frame as drawn (lines=\"plan\") against the cheapest frame", the_frame_as_drawn_against_the_cheapest)
