# The tests of @cadpole/plan_audit.star: each rule family on synthetic plans built to fail it and to pass it,
# then a side-hall plan (a synthetic 25-ft lot) rebuilt as a plan value and audited, mark for mark.
# `cadpole test stdlib/plan_audit_test.star`. Every plan here is made up; none is a real site.

load("@cadpole/plan.star", "court", "door", "fixture", "floor", "plan", "room", "site", "stack", "toy_house", "validate", "window")
load(
    "@cadpole/plan_audit.star",
    "audit",
    "code_rows",
    "door_rows",
    "filler_rows",
    "ftin",
    "furnish_rows",
    "isovist",
    "isovist_rows",
    "inches",
    "redpen",
    "row_line",
    "stack_rows",
    "walks",
)

def _mk(rooms, doors = None, windows = None, fixtures = None, stacks = None, courts = None, floors = None, lot = (0.0, 40.0, 0.0, 80.0), ceiling = 8.0):
    fl = floors or [floor("F1", z = 0.0, ceiling = ceiling)]
    return plan("t", site = site(lot, street = "S"), floors = fl, rooms = rooms, doors = doors or [], windows = windows or [],
                fixtures = fixtures or [], stacks = stacks or [], courts = courts or [])

def _R(key, kind, rect, fl = "F1", **kw):
    return room(key, key.replace("_", " ").capitalize(), floor = fl, kind = kind, rect = rect, **kw)

def _D(key, a, b, wall, at, fl = "F1", **kw):
    return door(key, floor = fl, a = a, b = b, wall = wall, at = at, **kw)

def _W(key, rk, wall, at, fl = "F1", **kw):
    return window(key, floor = fl, room = rk, wall = wall, at = at, **kw)

def _row(rows, key):
    hits = [r for r in rows if r["key"] == key]
    if len(hits) != 1:
        fail("%d rows keyed %s; the keys: %s" % (len(hits), key, ", ".join([r["key"] for r in rows])))
    return hits[0]

def _expect(rows, key, ok, has = None, severity = None):
    r = _row(rows, key)
    assert_eq(r["ok"], ok, msg = row_line(r))
    if has != None:
        assert_true(has in r["detail"], msg = "%r not in: %s" % (has, row_line(r)))
    if severity != None:
        assert_eq(r["severity"], severity, msg = row_line(r))
    return r

def _none(rows, prefix):
    hits = [r["key"] for r in rows if r["key"].startswith(prefix)]
    assert_eq(hits, [], msg = "no rows wanted under " + prefix)

# ---------------------------------------------------------------- formatting
def formatting():
    assert_eq(ftin(7.4167), "7'-5\"")
    assert_eq(ftin(2.0 + 11.0 / 12 + 15.0 / 192, 16), "2'-11 15/16\"")
    assert_eq(ftin(-0.5), "-0'-6\"")
    assert_eq(inches(1.0 / 192), "1/16 in")
    assert_eq(inches(35.9375 / 12), "35 15/16 in")

test("feet-inches and inches, to the sixteenth", formatting)

# ================================================================ code
def r304():
    p = _mk([
        _R("ok", "bed", (0.0, 9.0, 0.0, 9.0)),  # 81 sq ft, 9 ft across
        _R("narrow", "bed", (10.0, 10.0 + 82.0 / 12, 0.0, 12.0)),  # 6'-10" wide
        _R("small", "den", (18.0, 26.0, 0.0, 8.5)),  # 68 sq ft
        _R("galley", "kitchen", (27.0, 33.0, 0.0, 12.0)),  # 6 ft wide, but kitchens are exempt from the 7 ft
    ])
    rows = code_rows(p)
    _expect(rows, "code:R304:F1:ok", True, "81 sq ft")
    _expect(rows, "code:R304:F1:narrow", False, "6'-10\" wide (R304.2 wants 7'-0\")", "fail")
    _expect(rows, "code:R304:F1:small", False, "68 sq ft (R304.1 wants 70 sq ft)")
    _expect(rows, "code:R304:F1:galley", True, "exempt")
    # kitchens are exempt from the 70 sq ft too; a kept (existing) room's shortfall is a note
    k = code_rows(_mk([_R("tiny", "kitchen", (0.0, 11.42, 0.0, 6.08)), _R("old", "den", (12.0, 18.0, 0.0, 10.0), kept = True)]))
    _expect(k, "code:R304:F1:tiny", True, "69.4 sq ft")
    _expect(k, "code:R304:F1:old", False, "an existing room, kept", "note")
    # a room in pieces: a 5'-11" strip on an 18'-11" piece is a 10-ft-deep room, read on the rectangle they make
    q = _mk([room("din", "Dining", floor = "F1", kind = "dining", rects = [(19.0, 41.22, 4.7, 10.6), (22.32, 41.22, 10.6, 14.7), (31.3, 39.3, 1.2, 4.2)])])
    _expect(code_rows(q), "code:R304:F1:din", True, "7 ft read on the widest rectangle its pieces make, 18'-11\" x 10'-0\"")

test("R304: 70 sq ft and 7 ft for habitable rooms", r304)

def r305():
    p = _mk([
        _R("bed", "bed", (0.0, 10.0, 0.0, 10.0)),
        _R("low", "den", (11.0, 21.0, 0.0, 10.0), ceiling = 6.0 + 10.0 / 12),
        _R("bath", "bath", (22.0, 28.0, 0.0, 8.0), ceiling = 6.0 + 8.0 / 12),
    ])
    rows = code_rows(p)
    _expect(rows, "code:R305:F1:low", False, "ceiling 6'-10\" (R305.1 wants 7'-0\")")
    _none(rows, "code:R305:F1:bath")  # 6'-8" is enough for a bath
    q = _mk([_R("bed", "bed", (0.0, 10.0, 0.0, 10.0)), _R("bath", "bath", (11.0, 17.0, 0.0, 8.0), ceiling = 6.0 + 8.0 / 12)])
    _expect(code_rows(q), "code:R305:F1:-", True, "the lowest ceiling 6'-8\"")

test("R305: ceilings 7 ft, baths 6'-8\"", r305)

def r310():
    fl = [floor("B", z = -10.0, ceiling = 8.0), floor("F1", z = 0.0, ceiling = 8.0), floor("F2", z = 10.0, ceiling = 8.0)]
    rooms = [
        _R("cellar", "storage", (0.0, 20.0, 0.0, 30.0), fl = "B"),  # a basement with no way out
        _R("b1", "bed", (0.0, 10.0, 0.0, 10.0), fl = "F2"),
        _R("b2", "bed", (10.4, 20.0, 0.0, 10.0), fl = "F2"),
        _R("b3", "bed", (20.4, 30.0, 0.0, 10.0), fl = "F2"),
        _R("b4", "bed", (0.0, 10.0, 10.4, 20.0), fl = "F2"),
        _R("b5", "bed", (10.4, 20.0, 10.4, 20.0), fl = "F2"),
        _R("b6", "bed", (20.4, 30.0, 10.4, 20.0), fl = "F2"),
        _R("b7", "bed", (0.0, 10.0, 20.4, 30.0), fl = "F2"),
        _R("b8", "bed", (20.4, 30.0, 20.4, 30.0), fl = "F2"),
        _R("g1", "bed", (0.0, 10.0, 0.0, 10.0), fl = "F1"),
    ]
    wins = [
        _W("w1", "b1", "S", (5.0, 0.0), fl = "F2", source = "street", egress = True, clear = (2.0, 3.0)),
        _W("w2", "b2", "S", (15.0, 0.0), fl = "F2", source = "street", egress = True, clear = (1.5, 3.0)),
        _W("w3", "b3", "S", (25.0, 0.0), fl = "F2", source = "street", egress = True, sill = 4.0, head = 7.0, clear = (2.5, 2.5)),
        _W("w4", "b4", "W", (0.0, 15.0), fl = "F2", source = "side", egress = True),
        _W("w5", "b5", "E", (20.0, 15.0), fl = "F2", source = "well:lw", egress = True, clear = (2.5, 3.0)),
        _W("w6", "b6", "E", (30.0, 15.0), fl = "F2", source = "slot:sl", egress = True, clear = (2.5, 3.0)),
        # a grade-floor opening needs 5.0 sq ft, one a floor up 5.7
        _W("wg1", "g1", "S", (5.0, 0.0), fl = "F1", source = "street", egress = True, clear = (2.0, 2.6)),
        _W("w8", "b8", "E", (30.0, 25.0), fl = "F2", source = "side", egress = True, clear = (2.0, 2.6)),
    ]
    cts = [court("lw", kind = "well", rect = (20.0, 20.4, 10.4, 20.0), base = "F1", top = 25.0),
           court("sl", kind = "slot", rect = (30.5, 34.0, 0.0, 30.0), base = "F1", top = 25.0, reaches = "street")]
    rows = code_rows(_mk(rooms, windows = wins, courts = cts, floors = fl))
    _expect(rows, "code:R310.1:F2:b1", True, "onto the street; 24 in x 36 in clear, 6.0 sq ft")
    _expect(rows, "code:R310.1:F2:b2", False, "4.5 sq ft net clear (wants 5.7 sq ft)", "fail")
    _expect(rows, "code:R310.1:F2:b3", False, "its sill 4'-0\" over the floor (R310.2.2 wants 44 in at most)")
    _expect(rows, "code:R310.1:F2:b4", False, "unknown until the net clear opening is drawn", "note")
    _expect(rows, "code:R310.1:F2:b5", False, "light well lw, which does not open to a public way", "fail")
    _expect(rows, "code:R310.1:F2:b6", False, "ask the building department: is the slot sl a yard or court that opens to a public way? (it reaches the street)", "note")
    _expect(rows, "code:R310.1:F2:b7", False, "B7 has no window or outside door to escape by", "fail")
    _expect(rows, "code:R310.1:F1:g1", True, "5.2 sq ft")
    _expect(rows, "code:R310.1:F2:b8", False, "5.2 sq ft net clear (wants 5.7 sq ft)")
    _expect(rows, "code:R310.1-basement:B:-", False, "B is a basement (its floor 10'-0\" below grade) with no window or door out")
    # a 4-ft crawl of storage is not a story, so not a basement
    crawl = code_rows(_mk([_R("bins", "storage", (0.0, 20.0, 0.0, 30.0), fl = "C")], floors = [floor("C", z = -12.0, ceiling = 4.33)]))
    _none(crawl, "code:R310.1-basement")

test("R310.1: escape openings, their size, sill and what they open onto; basements", r310)

def r311_6():
    rows = code_rows(_mk([
        _R("hall", "hall", (0.0, 2.0 + 11.0 / 12 + 15.0 / 192, 0.0, 20.0)),
        _R("wide", "hall", (5.0, 8.0, 0.0, 20.0)),
    ]))
    _expect(rows, "code:R311.6:F1:hall", False, "Hall 2'-11 15/16\" clear, 1/16 in short of R311.6's 3'-0\"")
    _expect(rows, "code:R311.6:F1:wide", True, "3'-0\" clear")

test("R311.6: halls 36 in clear, to the sixteenth", r311_6)

def r311_7():
    rooms = [
        _R("st", "stair", (0.0, 2.875, 0.0, 12.0), core = "a"),  # a 2'-10 1/2" flight
        _R("st2", "stair", (10.0, 13.5, 0.0, 12.0), core = "b"),
        _R("land", "landing", (10.0, 15.0, 12.4, 14.9)),  # 2'-6" past the stair's end
        _R("land2", "landing", (0.0, 4.0, 12.4, 15.4)),  # 3'-0"
        _R("bath", "bath", (15.4, 22.0, 12.4, 18.0)),
    ]
    doors = [_D("d1", "land", "st2", "S", (11.75, 12.2), swing = "open"), _D("d2", "bath", "land", "W", (15.2, 13.6))]
    rows = code_rows(_mk(rooms, doors))
    _expect(rows, "code:R311.7-width:F1:st", False, "2'-10 1/2\" wide (R311.7.1 wants 3'-0\")")
    _expect(rows, "code:R311.7-width:F1:st2", True, "3'-6\" wide")
    # a U-stair's flights, from the plan's flight_width, not its rect's 6'-6"
    u = code_rows(_mk([_R("ust", "stair", (0.0, 9.67, 0.0, 6.5), flight_width = 3.15)]))
    _expect(u, "code:R311.7-width:F1:ust", True, "3'-1 13/16\" wide")
    _expect(rows, "code:R311.7-landing:F1:land", False, "2'-6\" deep in the direction of travel (R311.7.6 wants 3'-0\")")
    _expect(rows, "code:R311.7-landing:F1:land2", True, "3'-0\" deep")
    # the landing is also the way to the bath, so it is a hallway: R311.6 reads it too
    _expect(rows, "code:R311.6:F1:land", False, "2'-6\" clear")

test("R311.7: stair width, and landings as wide as the stair and 36 in deep", r311_7)

def cpc_402_5():
    wc = (5.0 / 3, 10.0 / 3, 0.0, 28.0 / 12)  # 20 x 28 in, its centre 2'-6" from either wall
    def bath(lav_v0, key):
        return _mk([_R(key, "bath", (0.0, 5.0, 0.0, 8.0))], fixtures = [
            fixture(key + "_wc", room = key, kind = "wc", rect = wc, front = "N"),
            fixture(key + "_lav", room = key, kind = "lav", rect = (0.5, 4.5, lav_v0, lav_v0 + 1.75), front = "S"),
        ])
    rows = code_rows(bath(28.0 / 12 + 21.0 / 12, "b21"))
    _expect(rows, "code:CPC402.5-b21_wc:F1:b21", False, "21 in clear in front; CPC 402.5 wants 24 in")
    assert_true("(that passes)" in _row(rows, "code:CPC402.5-b21_wc:F1:b21")["detail"], msg = "the IRC's 21 in holds")
    rows = code_rows(bath(28.0 / 12 + 2.0, "b24"))
    _expect(rows, "code:CPC402.5-b24_wc:F1:b24", True, "24 in clear in front")
    # a WC 12 in from its centre to the wall
    p = _mk([_R("tight", "wc", (0.0, 3.0, 0.0, 6.0))], fixtures = [fixture("t_wc", room = "tight", kind = "wc", rect = (0.17, 1.83, 0.0, 28.0 / 12), front = "N")])
    _expect(code_rows(p), "code:CPC402.5-t_wc:F1:tight", False, "12 in to a side")
    # a WC compartment with no fixture drawn, by its size
    rows = code_rows(_mk([_R("wcok", "wc", (0.0, 2.5, 0.0, 4.5)), _R("wcbad", "wc", (3.0, 5.4, 0.0, 4.5))]))
    _expect(rows, "code:CPC402.5-room:F1:wcok", True, "holds a 30-in WC space")
    _expect(rows, "code:CPC402.5-room:F1:wcbad", False, "does not hold")

test("CPC 402.5: the WC's 30 in and 24 in clear (the IRC's 21 in reported beside it)", cpc_402_5)

def showers():
    p = _mk([_R("ba", "bath", (0.0, 12.0, 0.0, 10.0))], fixtures = [
        fixture("s30", room = "ba", kind = "shower", rect = (0.0, 2.5, 0.0, 2.5)),
        fixture("s32", room = "ba", kind = "shower", rect = (3.0, 3.0 + 32.0 / 12, 0.0, 32.0 / 12)),
        fixture("s28", room = "ba", kind = "shower", rect = (7.0, 7.0 + 28.0 / 12, 0.0, 3.5)),
    ])
    rows = code_rows(p)
    _expect(rows, "code:shower-s30:F1:ba", False, "900 sq in")
    _expect(rows, "code:shower-s32:F1:ba", True, "1,024 sq in")
    _expect(rows, "code:shower-s28:F1:ba", False, "28 in x 42 in")

test("Showers: 30 in across and 1,024 sq in", showers)

def r302_5_1():
    rooms = [_R("garage", "garage", (0.0, 12.0, 0.0, 22.0)), _R("bed", "bed", (12.4, 24.0, 0.0, 12.0)), _R("mud", "mud", (12.4, 18.0, 12.4, 22.0))]
    doors = [_D("g_bed", "garage", "bed", "E", (12.2, 6.0)), _D("g_mud", "garage", "mud", "E", (12.2, 17.0))]
    rows = code_rows(_mk(rooms, doors))
    _expect(rows, "code:R302.5.1:F1:bed", False, "g_bed opens from the garage into bed", "fail")
    _expect(rows, "code:R302.5.1:F1:mud", True, "self-closing", "note")

test("R302.5.1: no door from the garage into a sleeping room", r302_5_1)

def r302_1():
    # a room whose west wall's outer face is 4 ft from the west lot line; walls 9 ft floor to floor (8-ft ceiling)
    def side(widths):
        ws = [_W("w%d" % i, "side", "W", (4.5, 13.0 + 6.0 * i), width = w, sill = 1.0, head = 7.0, source = "side") for i, w in enumerate(widths)]
        return code_rows(_mk([_R("side", "den", (4.5, 20.0, 10.0, 30.0))], windows = ws, lot = (0.0, 30.0, 0.0, 60.0)))
    rows = side([4.5, 4.5])  # 54 sq ft of glass in a wall 21 ft long (its outer face, corner to corner) and 9 ft high
    r = _expect(rows, "code:R302.1-W-5ft:F1:-", False, "4'-0\" from the lot line: openings 29% of its wall")
    assert_true("ask the building department" in r["detail"] and "Table R302.1(2)" in r["detail"], msg = "the sprinklered table is reported")
    assert_true(r.get("at") != None, msg = "a failing wall is marked at its first opening")
    _expect(side([4.0]), "code:R302.1-W-5ft:F1:-", True, "13%")
    # glass 1'-6" from the line: none allowed
    rows = code_rows(_mk([_R("close", "den", (2.0, 20.0, 10.0, 30.0))], windows = [_W("wc", "close", "W", (2.0, 20.0), source = "side")], lot = (0.0, 30.0, 0.0, 60.0)))
    _expect(rows, "code:R302.1-W-3ft:F1:-", False, "1'-6\" from the lot line: openings wc")
    # a window onto a side slot 4 ft wide is as close to the line as the wall it is in
    sl = court("sl", kind = "slot", rect = (0.5, 4.5, 10.0, 30.0), base = "F1", top = 20.0, reaches = "street")
    rows = code_rows(_mk([_R("slotroom", "den", (5.0, 20.0, 10.0, 30.0))], windows = [_W("ws", "slotroom", "W", (5.0, 20.0), width = 9.0, sill = 1.0, head = 7.0, source = "slot:sl")],
                         courts = [sl], lot = (0.0, 30.0, 0.0, 60.0)))
    _expect(rows, "code:R302.1-W-5ft:F1:-", False, "4'-6\" from the lot line: openings 29% of its wall")

test("R302.1: openings against the fire separation distance, both tables", r302_1)

# ================================================================ doors
def _street_house(garage_door, bath_door_to = "garage"):
    """An entry and hall off the street, a garage off the street beside them, a bath and a den."""
    rooms = [
        _R("entry", "entry", (0.0, 6.0, 0.0, 6.0)),
        _R("hall", "hall", (0.0, 3.5, 6.4, 30.0)),
        _R("den", "den", (3.9, 15.0, 6.4, 18.0)),
        _R("garage", "garage", (6.4, 18.0, 0.0, 6.0)),
        _R("gbath", "bath", (15.4, 22.0, 6.4, 12.0)),
    ]
    doors = [
        _D("d_front", "entry", "outside:street", "S", (3.0, 0.0)),
        _D("d_hall", "hall", "entry", "S", (1.75, 6.2), swing = "open"),
        _D("d_den", "den", "hall", "W", (3.7, 12.0)),
        _D("d_gar", "garage", "outside:street", "S", (12.0, 0.0), width = 9.0),
        _D("d_gbath", "gbath", bath_door_to, "S" if bath_door_to == "garage" else "W", (18.0, 6.2) if bath_door_to == "garage" else (15.2, 9.0)),
    ]
    if garage_door:
        doors.append(_D("d_gh", "garage", "entry", "W", (6.2, 3.0)))
    return _mk(rooms, doors)

def garage_and_baths():
    rows = door_rows(_street_house(False))
    _expect(rows, "doors:garage-door:F1:garage", False, "No door from the garage into the house", "fail")
    _expect(rows, "doors:bath-reach:F1:gbath", False, "Gbath: its only door is from the garage")
    rows = door_rows(_street_house(True, "den"))
    _expect(rows, "doors:garage-door:F1:garage", True, "opens into entry")
    _none(rows, "doors:bath-reach")

test("Doors: a door from the garage into the house; no bath reached only through the garage", garage_and_baths)

def baths_through():
    rooms = [
        _R("entry", "entry", (0.0, 6.0, 0.0, 6.0)),
        _R("hall", "hall", (0.0, 3.5, 6.4, 40.0)),
        _R("bed_a", "bed", (3.9, 15.0, 6.4, 18.0)),
        _R("bed_b", "bed", (15.4, 26.0, 6.4, 18.0)),
        _R("bath_b", "bath", (26.4, 32.0, 6.4, 12.0)),  # off bedroom b, which is reached only through bedroom a
        _R("primary", "bed", (3.9, 15.0, 18.4, 30.0)),
        _R("pcl", "closet", (15.4, 21.0, 18.4, 23.0)),
        _R("pbath", "bath", (21.4, 28.0, 18.4, 25.0)),  # through the primary's closet
        _R("suite", "bed", (3.9, 15.0, 30.4, 40.0)),
        _R("sbath", "bath", (15.4, 21.0, 30.4, 36.0)),  # straight off its bedroom: fine
    ]
    doors = [
        _D("d_front", "entry", "outside:street", "S", (3.0, 0.0)),
        _D("d_hall", "hall", "entry", "S", (1.75, 6.2), swing = "open"),
        _D("d_a", "bed_a", "hall", "W", (3.7, 12.0)),
        _D("d_ab", "bed_b", "bed_a", "W", (15.2, 12.0)),
        _D("d_bb", "bath_b", "bed_b", "W", (26.2, 9.0)),
        _D("d_p", "primary", "hall", "W", (3.7, 24.0)),
        _D("d_pcl", "pcl", "primary", "W", (15.2, 20.7)),
        _D("d_pb", "pbath", "pcl", "W", (21.2, 20.7)),
        _D("d_s", "suite", "hall", "W", (3.7, 35.0)),
        _D("d_sb", "sbath", "suite", "W", (15.2, 33.0)),
    ]
    rows = door_rows(_mk(rooms, doors))
    _expect(rows, "doors:bath-reach:F1:bath_b", False, "Bath b reached only through the bed a (11'-1\" x 11'-7\")")
    _expect(rows, "doors:bath-reach:F1:pbath", False, "Pbath reached only through the pcl (5'-7\" x 4'-7\")")
    _none(rows, "doors:bath-reach:F1:sbath")
    _expect(rows, "doors:bed-off-day:F1:bed_b", False, "Bed b opens off the bed a", "warn")

test("Doors: no bath reached only through a closet or another bedroom", baths_through)

def powder_and_beds():
    rooms = [
        _R("entry", "entry", (0.0, 6.0, 0.0, 6.0)),
        _R("hall", "hall", (0.0, 3.5, 6.4, 30.0)),
        _R("living", "living", (3.9, 15.0, 0.0, 14.0)),
        _R("dining", "dining", (3.9, 15.0, 14.4, 24.0)),
        _R("pw1", "powder", (15.4, 20.0, 14.4, 19.0)),
        _R("pw2", "powder", (3.9, 8.5, 24.4, 30.0)),
        _R("bed", "bed", (15.4, 26.0, 0.0, 14.0)),
    ]
    doors = [
        _D("d_front", "entry", "outside:street", "S", (3.0, 0.0)),
        _D("d_hall", "hall", "entry", "S", (1.75, 6.2), swing = "open"),
        _D("d_liv", "living", "hall", "W", (3.7, 10.0), swing = "open"),
        _D("d_din", "dining", "hall", "W", (3.7, 20.0), swing = "open"),
        _D("d_pw1", "pw1", "dining", "W", (15.2, 16.5)),
        _D("d_pw2", "pw2", "hall", "W", (3.7, 27.0)),
        _D("d_bed", "bed", "living", "W", (15.2, 7.0)),
    ]
    rows = door_rows(_mk(rooms, doors))
    _expect(rows, "doors:powder:F1:pw1", False, "Pw1 opens into the dining", "warn")
    _expect(rows, "doors:powder:F1:pw2", True, "opens off hall")
    _expect(rows, "doors:bed-off-day:F1:bed", False, "Bed opens off the living", "warn")

test("Doors: the powder room off circulation; bedrooms off day rooms warn", powder_and_beds)

def pantry_reach_and_halls():
    rooms = [
        _R("entry", "entry", (0.0, 12.0, 0.0, 6.0)),
        _R("kitchen", "kitchen", (0.0, 12.0, 6.4, 18.0)),
        _R("hall", "hall", (12.4, 15.4, 0.0, 40.0)),
        _R("pantry", "pantry", (16.0, 20.0, 36.0, 40.0)),
        _R("den", "den", (16.0, 26.0, 0.0, 12.0)),
        _R("lost", "study", (0.0, 10.0, 30.0, 40.0)),  # no door at all
        _R("pantry2", "pantry", (0.0, 4.0, 18.4, 22.0)),
    ]
    doors = [
        _D("d_front", "entry", "outside:street", "S", (6.0, 0.0)),
        _D("d_ek", "kitchen", "entry", "S", (6.0, 6.2), swing = "open"),
        _D("d_kh", "kitchen", "hall", "E", (12.2, 12.0)),
        _D("d_hp", "pantry", "hall", "W", (15.7, 38.0)),
        _D("d_den", "den", "hall", "W", (15.7, 6.0)),
        _D("d_kp", "pantry2", "kitchen", "S", (2.0, 18.2)),
    ]
    p = _mk(rooms, doors)
    rows = door_rows(p)
    # the kitchen's centre (6, 12.2) > its door (12.2, 12) > the pantry's door (15.7, 38) > its centre (18, 38)
    w = walks(p, "F1", "kitchen", (6.0, 12.2), ["pantry"])["pantry"]
    assert_close(w, 6.2 + 0.2 + 3.5 + 26.0 + 2.3, tol = 1e-9)
    _expect(rows, "doors:pantry:F1:pantry", False, "Pantry: a 38'-2\" walk from the kitchen", "warn")
    _expect(rows, "doors:reach:F1:lost", False, "Lost: no way to it from the entry", "fail")
    # the hall runs 40 ft for the den (the kitchen and the entry count; the pantry does not) and dead-ends past it
    _expect(rows, "doors:hall:F1:hall", False, "28'-0\" past its last door")
    _expect(rows, "doors:pantry:F1:pantry2", True, "Pantry2 opens off the kitchen")
    # a pantry-kind room on a floor with no kitchen (a bar, a store) is a note
    lone = door_rows(_mk([_R("bar", "pantry", (0.0, 4.0, 0.0, 8.0), fl = "F2")], floors = [floor("F2", z = 10.0)]))
    _expect(lone, "doors:pantry:F2:bar", True, "no kitchen on F2", "note")

test("Doors: the pantry off the kitchen, its walk through the doors, reach and dead-end halls", pantry_reach_and_halls)

def kitchen_doors_and_walks():
    rooms = [
        _R("entry", "entry", (0.0, 6.0, 0.0, 6.0)),
        _R("hall", "hall", (6.4, 9.4, 0.0, 30.0)),
        _R("kitchen", "kitchen", (9.8, 22.0, 0.0, 12.0)),
        _R("dining", "dining", (22.4, 32.0, 0.0, 12.0)),
        _R("family", "family", (9.8, 22.0, 12.4, 24.0)),
        _R("st", "stair", (0.0, 6.0, 6.4, 18.0)),
        _R("bed", "bed", (9.8, 22.0, 24.4, 34.0)),
    ]
    doors = [
        _D("d_front", "entry", "outside:street", "S", (3.0, 0.0)),
        _D("d_eh", "hall", "entry", "W", (6.2, 3.0), swing = "open"),
        _D("d_k1", "kitchen", "hall", "W", (9.6, 6.0)),
        _D("d_k2", "kitchen", "dining", "E", (22.2, 6.0)),
        _D("d_k3", "kitchen", "family", "N", (16.0, 12.2)),
        _D("d_k4", "kitchen", "outside:street", "S", (16.0, 0.0)),
        _D("d_st", "st", "hall", "E", (6.2, 12.0), swing = "open"),
        _D("d_bed", "bed", "hall", "W", (9.6, 29.0)),
    ]
    p = _mk(rooms, doors)
    rows = door_rows(p)
    _expect(rows, "doors:kitchen-doors:F1:kitchen", False, "Kitchen: 4 doors", "warn")
    # open plan is no door: the dining's cased opening leaves the kitchen 3 doors
    q = _mk(rooms, [x if x["key"] != "d_k2" else dict(x, swing = "open") for x in doors])
    _expect(door_rows(q), "doors:kitchen-doors:F1:kitchen", True, "Kitchen: 3 doors (hall, family, outside); open to the dining")
    # from the stair's opening (6.2, 12) to the bedroom's door (9.6, 29), then to its centre (15.9, 29.2)
    _expect(rows, "doors:walks:F1:-", True, "Bed 26'-11\"", "note")

test("Doors: the kitchen's door count, and walks from the stair to the bedrooms", kitchen_doors_and_walks)

def the_toy_house_audits():
    A = audit(toy_house(), light = False, furnish = False)
    assert_true(len(A["rows"]) > 30, msg = "rows from every family")
    assert_eq([r for r in A["rows"] if r["key"].startswith("furnish:")], [], msg = "furnish=False leaves the re-pack out")
    for r in A["rows"]:
        for k in ("key", "name", "ok", "detail", "rule", "severity", "floor", "room"):
            assert_true(k in r, msg = "%s has no %s" % (r["key"], k))
        assert_true(r["severity"] in ("fail", "warn", "note"), msg = row_line(r))
    for m in A["marks"]:
        assert_true(m["n"] > 0 and (m.get("at") != None or m["room"] == None), msg = str(m))
    L = audit(toy_house(), name = "t_toy")
    assert_true(len([r for r in L["rows"] if r["key"].startswith("light:share:")]) == 2, msg = "a light share for each floor")
    # the toy house names its bedrooms' sets (queen, twin, king): each re-packed at its drawn size
    fr = {r["room"]: r for r in L["rows"] if r["key"].startswith("furnish:repack:")}
    assert_eq(sorted(fr.keys()), ["bed1", "bed2", "primary"])
    return len(A["rows"])

test("The toy house: every family runs, every row has the contract's keys", the_toy_house_audits)

# ================================================================ stacks
def stacks():
    fl = [floor("F1", z = 0.0), floor("F2", z = 10.0), floor("F3", z = 20.0)]
    rooms = [
        _R("k1", "kitchen", (0.0, 12.0, 0.0, 12.0), fl = "F1"),
        _R("liv1", "living", (12.4, 30.0, 0.0, 20.0), fl = "F1"),
        _R("hall1", "hall", (0.0, 3.5, 12.4, 30.0), fl = "F1"),
        _R("gar", "garage", (3.9, 15.0, 20.4, 40.0), fl = "F1"),
        _R("bath2", "bath", (0.0, 8.0, 0.0, 8.0), fl = "F2"),
        _R("bed2", "bed", (8.4, 20.0, 0.0, 12.0), fl = "F2"),
        _R("ldry", "laundry", (20.4, 26.0, 0.0, 6.0), fl = "F2"),
        _R("bath3", "bath", (0.0, 8.0, 0.0, 8.0), fl = "F3"),
        _R("cl3", "closet", (8.4, 12.0, 0.0, 6.0), fl = "F3"),
    ]
    sts = [
        stack("s_wall", at = (12.2, 6.0), floors = ["F1"]),  # in the wall between the kitchen and the living room
        stack("s_wet", at = (8.2, 4.0), floors = ["F2", "F3"]),  # in both baths' east walls
        stack("s_day", at = (20.0, 10.0), floors = ["F1"]),  # in the open in the living room
        stack("s_hall", at = (1.75, 20.0), floors = ["F1"]),  # down the middle of the hall
        stack("s_gar", at = (9.0, 30.0), floors = ["F1"]),  # in the garage's clear space
        stack("s_bed", at = (14.0, 6.0), floors = ["F2"]),  # in the open in a bedroom: a warning
        stack("s_skip", at = (12.2, 4.0), floors = ["F1", "F3"]),  # in walls, but not on F2
        stack("s_out", at = (40.0, 40.0), floors = ["F2"]),  # nowhere near a room
    ]
    rows = stack_rows(_mk(rooms, stacks = sts, floors = fl))
    _expect(rows, "stacks:wet:F1:k1", True, "K1 on s_wall")
    _expect(rows, "stacks:wet:F2:bath2", True, "on s_wet")
    _expect(rows, "stacks:wet:F3:bath3", True, "on s_wet")
    _expect(rows, "stacks:wet:F2:ldry", False, "Ldry: no stack within 0'-7\" of its walls", "fail")
    _expect(rows, "stacks:stack-s_wall:F1:-", True, "s_wall in a wall on F1")
    _expect(rows, "stacks:stack-s_day:F1:liv1", False, "s_day stands in the liv1 on F1", "fail")
    _expect(rows, "stacks:stack-s_hall:F1:hall1", False, "the hall's clear width", "fail")
    _expect(rows, "stacks:stack-s_gar:F1:gar", False, "the garage's clear space", "fail")
    _expect(rows, "stacks:stack-s_bed:F2:bed2", False, "stands in bed2", "warn")
    _expect(rows, "stacks:stack-s_skip:F1:-", False, "s_skip skips F2")
    # a floor between a stack's ends that is not built under it (a separate building's floor beside the house) is no gap
    rows2 = stack_rows(_mk([_R("a1", "den", (0.0, 12.0, 0.0, 12.0), fl = "A"), _R("a2", "den", (12.4, 24.0, 0.0, 12.0), fl = "A"),
                            _R("b1", "den", (0.0, 12.0, 0.0, 12.0), fl = "B"), _R("b2", "den", (12.4, 24.0, 0.0, 12.0), fl = "B"),
                            _R("cot", "den", (50.0, 60.0, 50.0, 60.0), fl = "C")],
                           stacks = [stack("sx", at = (12.2, 6.0), floors = ["A", "B"])],
                           floors = [floor("A", z = 0.0), floor("C", z = 5.0), floor("B", z = 10.0)]))
    _expect(rows2, "stacks:stack-sx:A:-", True, "sx in a wall on A, B")
    _expect(rows, "stacks:stack-s_out:F2:-", False, "s_out has no wall to run in on F2")
    # an implied drain (the plan draws no stack, the adapter marks where one must be) passes as a note
    imp = stack_rows(_mk(rooms, stacks = sts + [stack("s_ld", at = (26.2, 3.0), floors = ["F2"], implied = True)], floors = fl))
    _expect(imp, "stacks:wet:F2:ldry", True, "Ldry on s_ld, an implied drain", "note")

test("Stacks: wet rooms on a stack in their walls; none in a day room, a hall, a garage; continuous", stacks)

# ================================================================ fillers
def fillers():
    rooms = [
        _R("cl_long", "closet", (0.0, 3.5, 0.0, 19.0 + 4.0 / 12)),  # 3'-6" x 19'-4": leftover depth
        _R("store", "storage", (3.9, 14.1, 0.0, 13.0)),  # 133 sq ft of storage
        _R("mud", "mud", (14.5, 20.1, 0.0, 19.6)),  # 5'-7" x 19'-7"
        _R("bed", "bed", (20.5, 30.7, 0.0, 20.4)),  # 10'-2" x 20'-5": 2:1
        _R("den", "den", (0.0, 12.0, 20.0, 32.0)),  # no window
        _R("study", "study", (12.4, 24.0, 20.8, 32.8)),  # a 2 x 3 ft window in 139 sq ft
    ]
    wins = [_W("w_bed", "bed", "E", (30.7, 10.0), width = 4.0, sill = 2.0, head = 7.0),
            _W("w_study", "study", "N", (18.0, 32.8), width = 2.0, sill = 4.0, head = 7.0)]
    p = _mk(rooms, windows = wins)
    rows = filler_rows(p, program = ["bed", "den", "study", "mud", {"kind": "closet", "count": 1}])
    _expect(rows, "fillers:unasked:F1:store", False, "the program did not ask for it", "warn")
    _expect(rows, "fillers:leftover:F1:cl_long", False, "3'-6\" x 19'-4\" cl long: leftover depth")
    _expect(rows, "fillers:leftover:F1:mud", False, "5'-7\" x 19'-7\" mud: leftover depth, a 110 sq ft filler")
    _expect(rows, "fillers:leftover:F1:store", False, "a 133 sq ft filler")
    _expect(rows, "fillers:shares:F1:-", False, "storage, closets and pantry 200 sq ft (25%)", "warn")
    _expect(rows, "fillers:window:F1:den", False, "Den has no window", "fail")
    _expect(rows, "fillers:window:F1:study", False, "Study: glass 4% of its floor", "warn")
    _expect(rows, "fillers:window:F1:bed", True)
    _expect(rows, "fillers:shape:F1:bed", False, "Bed 10'-2\" x 20'-5\": 2.0:1", "warn")
    # no program: nothing is "unasked"; a square room is no mark
    rows = filler_rows(p)
    _none(rows, "fillers:unasked")
    _none(rows, "fillers:shape:F1:den")

test("Fillers: unasked rooms, leftover depth, storage and hall shares, windowless rooms, 2:1 rooms", fillers)

# ================================================================ the marks
def marks_for_plan_draw():
    rows = [
        {"key": "a", "name": "A", "ok": False, "detail": "first finding; more", "rule": "R1", "severity": "fail", "floor": "F2", "room": None, "at": (1.0, 2.0)},
        {"key": "b", "name": "B", "ok": False, "detail": "ask the building department: is it?", "rule": "R2", "severity": "note", "floor": "F1", "room": None, "rect": (0.0, 10.0, 0.0, 10.0)},
        {"key": "c", "name": "C", "ok": False, "detail": "unknown: no size", "rule": "R3", "severity": "note", "floor": "F1", "room": None, "at": (5.0, 5.0)},
        {"key": "d", "name": "D", "ok": True, "detail": "fine", "rule": "R4", "severity": "fail", "floor": "F1", "room": None, "at": (5.0, 5.0)},
        {"key": "e", "name": "E", "ok": False, "detail": "a floor's share", "rule": "R5", "severity": "warn", "floor": "F1", "room": None},
    ]
    p = _mk([_R("r1", "den", (0.0, 10.0, 0.0, 10.0)), _R("r2", "den", (0.0, 10.0, 0.0, 10.0), fl = "F2")],
            floors = [floor("F1", z = 0.0), floor("F2", z = 10.0)])
    m = redpen(rows, p)
    # the lower floor first; the building department's question drawn as a warning; the unknown and the pass left
    # out; the placeless row
    # numbered and listed, with no ring
    assert_eq([x["of"] for x in m], ["b", "e", "a"])
    assert_eq([x["n"] for x in m], [1, 2, 3])
    assert_eq(m[1].get("at"), None)
    m = [x for x in m if x["of"] != "e"]
    assert_eq(m[0]["kind"], "ask")
    assert_eq(m[0]["severity"], "warn")
    assert_eq(m[1]["name"], "first finding")
    assert_eq(m[1]["detail"], "more")
    assert_eq(m[0]["detail"], "B")  # nothing after the finding: the rule's name stands in
    assert_eq(m[0]["at"], (1.3, 8.4))  # a rect's mark sits near its north-west corner

test("redpen(): the rows plan_draw draws, numbered floor by floor", marks_for_plan_draw)

# ================================================================ a side hall, rebuilt and audited
# A side-hall plan on a 25-ft lot, as a solver scoring area alone leaves it, written out as a plan value. The
# audit marks eleven rooms in it and the test holds it to those marks. One more: the 133-sq-ft storage room is a filler.
# Bedroom 3 is the furniture re-pack's (@cadpole/plan_furnish.star): 7'-5" wide holds the full set at no depth.
# Bedroom 2 ("8'-1" deep, plus the bay") looks too shallow and is not marked: plan_furnish packs a room
# of several rects in its bounding box, and the full set fits with the bed partly in the bay (its door swings in);
# read with its bay the room is 9'-0" x 11'-7", under 2:1.

def side_hall_plan():
    """A side-hall parti as a plan: a synthetic 25 x 100-ft lot, three floors, lot coordinates (the rooms' inside
    faces 6 in off the side lines and 5 ft 6 in back from the street line). Doors at the partitions' centres; windows
    on the facades and the court; the rear-arm court; 32-ft neighbours either side. Nothing in it is a real site."""
    G, M, U = "G", "M", "U"
    rooms = [
        room("vestibule", "Vestibule", floor = G, kind = "entry", rect = (0.5, 7.9, 5.5, 10.721)),
        room("garage", "Garage", floor = G, kind = "garage", rect = (8.3, 18.5, 5.5, 25.5)),
        room("stairG", "Stair", floor = G, kind = "stair", rect = (0.5, 4, 11.121, 26.621), core = "st"),
        room("hallG", "Hall", floor = G, kind = "hall", rect = (4.4, 7.9, 11.121, 55.293)),
        room("clG1", "Closet", floor = G, kind = "closet", rect = (0.5, 4, 27.021, 46.321)),
        room("clG2", "Closet", floor = G, kind = "closet", rect = (0.5, 4, 46.721, 55.293)),
        room("mud", "Mudroom", floor = G, kind = "mud", rect = (18.9, 24.5, 19.293, 38.893)),
        room("bathG", "Bath", floor = G, kind = "bath", rect = (18.9, 24.5, 5.5, 18.893)),
        room("storeG", "Storage", floor = G, kind = "storage", rect = (8.3, 18.5, 25.9, 38.893)),
        room("den", "Den / flex", floor = G, kind = "den", rect = (8.3, 24.5, 39.293, 55.293)),
        room("stairM", "Stair", floor = M, kind = "stair", rect = (0.5, 4, 11.121, 29.621), core = "st"),
        room("hallM", "Hall", floor = M, kind = "hall", rect = (4.4, 7.9, 5.5, 46.992)),
        room("living", "Living", floor = M, kind = "living", rects = [(8.3, 24.5, 5.5, 18.843), (11.9, 20.9, 2.0, 5.5)]),
        room("dining", "Dining", floor = M, kind = "dining", rect = (8.3, 18.5, 19.243, 31.762)),
        room("kitchen", "Kitchen", floor = M, kind = "kitchen", rect = (8.3, 18.5, 32.162, 46.992)),
        room("powder", "Powder", floor = M, kind = "powder", rect = (18.9, 24.5, 19.243, 26.362)),
        room("pantry", "Pantry", floor = M, kind = "pantry", rect = (0.5, 4, 38.992, 46.992)),
        room("clM1", "Closet", floor = M, kind = "closet", rect = (0.5, 4, 5.5, 10.721)),
        room("clM2", "Closet", floor = M, kind = "closet", rect = (0.5, 4, 30.021, 38.592)),
        room("stairU", "Stair", floor = U, kind = "stair", rect = (0.5, 4, 14.121, 29.621), core = "st"),
        room("hallU", "Hall", floor = U, kind = "hall", rect = (4.4, 7.9, 5.5, 33.792)),
        room("bed2", "Bedroom 2", floor = U, kind = "bed", rects = [(8.3, 24.5, 5.5, 13.6), (11.9, 20.9, 2.0, 5.5)]),
        room("bed3", "Bedroom 3", floor = U, kind = "bed", rect = (0.5, 7.9, 34.192, 48.992)),
        room("primary", "Primary bedroom", floor = U, kind = "bed", rect = (8.3, 18.5, 28.592, 48.992)),
        room("bath2", "Bath 2", floor = U, kind = "bath", rect = (8.3, 18.5, 14, 19.392)),
        room("laundry", "Laundry", floor = U, kind = "laundry", rect = (8.3, 18.5, 19.792, 23.292)),
        room("pcloset", "Primary closet", floor = U, kind = "closet", rect = (8.3, 18.5, 23.692, 28.192)),
        room("pbath", "Primary bath", floor = U, kind = "bath", rect = (18.9, 24.5, 14, 26.362)),
        room("clU1", "Closet", floor = U, kind = "closet", rect = (0.5, 4, 30.021, 33.792)),
        room("clU2", "Closet", floor = U, kind = "closet", rect = (0.5, 4, 5.5, 13.721)),
        room("deck", "Deck", floor = M, kind = "deck", rect = (0.5, 24.5, 47.492, 55.792)),
    ]
    doors = [
        door("d1_vestibule_out_street", floor = G, a = "vestibule", b = "outside:street", wall = "S", at = (4.2, 5.5), width = 3, swing = "a"),
        door("d2_garage_out_street", floor = G, a = "garage", b = "outside:street", wall = "S", at = (13.4, 5.5), width = 9, swing = "a"),
        door("d3_hallG_vestibule", floor = G, a = "hallG", b = "vestibule", wall = "S", at = (6.15, 10.921), width = 2.67, swing = "a"),
        door("d4_clG1_hallG", floor = G, a = "clG1", b = "hallG", wall = "E", at = (4.2, 36.671), width = 2.67, swing = "a"),
        door("d5_clG2_hallG", floor = G, a = "clG2", b = "hallG", wall = "E", at = (4.2, 51.007), width = 2.67, swing = "a"),
        door("d6_mud_garage", floor = G, a = "mud", b = "garage", wall = "W", at = (18.7, 22.396), width = 2.67, swing = "a"),
        door("d7_bathG_garage", floor = G, a = "bathG", b = "garage", wall = "W", at = (18.7, 12.196), width = 2.67, swing = "a"),
        door("d8_storeG_hallG", floor = G, a = "storeG", b = "hallG", wall = "W", at = (8.1, 32.396), width = 2.67, swing = "a"),
        door("d9_den_hallG", floor = G, a = "den", b = "hallG", wall = "W", at = (8.1, 47.293), width = 2.67, swing = "a"),
        door("d10_hallM_stairM", floor = M, a = "hallM", b = "stairM", wall = "W", at = (4.2, 28.121), width = 3, swing = "open"),
        door("d11_living_hallM", floor = M, a = "living", b = "hallM", wall = "W", at = (8.1, 12.171), width = 2.67, swing = "a"),
        door("d12_dining_living", floor = M, a = "dining", b = "living", wall = "S", at = (13.4, 19.043), width = 2.67, swing = "a"),
        door("d13_kitchen_dining", floor = M, a = "kitchen", b = "dining", wall = "S", at = (13.4, 31.962), width = 2.67, swing = "a"),
        door("d14_powder_dining", floor = M, a = "powder", b = "dining", wall = "W", at = (18.7, 22.803), width = 2.67, swing = "a"),
        door("d15_pantry_hallM", floor = M, a = "pantry", b = "hallM", wall = "E", at = (4.2, 42.992), width = 2.67, swing = "a"),
        door("d16_clM1_hallM", floor = M, a = "clM1", b = "hallM", wall = "E", at = (4.2, 8.11), width = 2.67, swing = "a"),
        door("d17_clM2_hallM", floor = M, a = "clM2", b = "hallM", wall = "E", at = (4.2, 34.306), width = 2.67, swing = "a"),
        door("d18_hallU_stairU", floor = U, a = "hallU", b = "stairU", wall = "W", at = (4.2, 28.121), width = 3, swing = "open"),
        door("d19_bed2_hallU", floor = U, a = "bed2", b = "hallU", wall = "W", at = (8.1, 9.55), width = 2.67, swing = "a"),
        door("d20_bed3_hallU", floor = U, a = "bed3", b = "hallU", wall = "S", at = (6.15, 33.992), width = 2.67, swing = "a"),
        door("d21_primary_hallU", floor = U, a = "primary", b = "hallU", wall = "W", at = (8.1, 31.192), width = 2.67, swing = "a"),
        door("d22_bath2_hallU", floor = U, a = "bath2", b = "hallU", wall = "W", at = (8.1, 16.696), width = 2.67, swing = "a"),
        door("d23_laundry_hallU", floor = U, a = "laundry", b = "hallU", wall = "W", at = (8.1, 21.542), width = 2.67, swing = "a"),
        door("d24_pcloset_primary", floor = U, a = "pcloset", b = "primary", wall = "N", at = (13.4, 28.392), width = 2.67, swing = "a"),
        door("d25_pbath_pcloset", floor = U, a = "pbath", b = "pcloset", wall = "W", at = (18.7, 25.027), width = 2.67, swing = "a"),
        door("d26_clU1_hallU", floor = U, a = "clU1", b = "hallU", wall = "E", at = (4.2, 31.906), width = 2.67, swing = "a"),
        door("d27_clU2_hallU", floor = U, a = "clU2", b = "hallU", wall = "E", at = (4.2, 9.61), width = 2.67, swing = "a"),
        door("d28_hallG_stairG", floor = G, a = "hallG", b = "stairG", wall = "W", at = (4.2, 12.621), width = 3, swing = "open"),
        door("d29_hallM_stairM", floor = M, a = "hallM", b = "stairM", wall = "W", at = (4.2, 12.621), width = 3, swing = "open"),
        door("d_garden", floor = G, a = "hallG", b = "outside:yard", wall = "N", at = (6.15, 55.293), width = 3.0, swing = "a"),
        door("d_deck", floor = M, a = "kitchen", b = "deck", wall = "N", at = (13.4, 46.992), width = 3.0, swing = "a"),
    ]
    windows = [
        window("w1_bathG", floor = G, room = "bathG", wall = "S", at = (21.7, 5.5), width = 3.36, sill = 4.0, head = 8.0, source = "street"),
        window("w2_den", floor = G, room = "den", wall = "N", at = (16.4, 55.293), width = 9.72, sill = 2.5, head = 8.0, source = "rear"),
        window("w3_hallM", floor = M, room = "hallM", wall = "S", at = (6.15, 5.5), width = 2, sill = 2.5, head = 8.0, source = "street"),
        window("w4_hallM", floor = M, room = "hallM", wall = "N", at = (6.15, 46.992), width = 2, sill = 2.5, head = 8.0, source = "rear"),
        window("w5_dining", floor = M, room = "dining", wall = "E", at = (18.5, 29.262), width = 3.5, sill = 2.5, head = 8.0, source = "court:arm"),
        window("w6_kitchen", floor = M, room = "kitchen", wall = "N", at = (13.4, 46.992), width = 6.12, sill = 2.5, head = 8.0, source = "rear"),
        window("w7_kitchen", floor = M, room = "kitchen", wall = "E", at = (18.5, 39.578), width = 10.381, sill = 2.5, head = 8.0, source = "court:arm"),
        window("w8_powder", floor = M, room = "powder", wall = "N", at = (21.7, 26.362), width = 3.92, sill = 4.0, head = 8.0, source = "court:arm"),
        window("w9_hallU", floor = U, room = "hallU", wall = "S", at = (6.15, 5.5), width = 2, sill = 2.5, head = 8.0, source = "street"),
        window("w10_bed3", floor = U, room = "bed3", wall = "N", at = (4.2, 48.992), width = 4.44, sill = 2.5, head = 8.0, source = "rear", egress = True),
        window("w11_primary", floor = U, room = "primary", wall = "N", at = (13.4, 48.992), width = 6.12, sill = 2.5, head = 8.0, source = "rear", egress = True),
        window("w12_primary", floor = U, room = "primary", wall = "E", at = (18.5, 38.792), width = 14.28, sill = 2.5, head = 8.0, source = "court:arm"),
        window("w13_pbath", floor = U, room = "pbath", wall = "N", at = (21.7, 26.362), width = 3.92, sill = 4.0, head = 8.0, source = "court:arm"),
        window("w_bay_living", floor = M, room = "living", wall = "S", at = (16.4, 2.0), width = 6.0, sill = 2.0, head = 8.0, source = "street"),
        window("w_bay_bed2", floor = U, room = "bed2", wall = "S", at = (16.4, 2.0), width = 6.0, sill = 2.0, head = 8.0, source = "street", egress = True),
    ]
    stacks = [
        stack("s1", at = (18.7, 19.592), floors = [G, M, U]),
        stack("s2", at = (18.7, 39.043), floors = [G, M, U]),
        stack("s3", at = (18.7, 19.093), floors = [G, M, U]),
    ]
    courts = [
        court("arm", kind = "court", rect = (18.9, 24.5, 26.762, 46.992), base = M, top = 32.0),
        court("arm_u", kind = "court", rect = (18.9, 24.5, 46.992, 48.992), base = U, top = 32.0),
    ]
    return plan(
        "side_hall",
        source = "side hall, 25-ft lot",
        site = site((0.0, 25.0, 0.0, 100.0), street = "S", setbacks = {"front": 5.0, "rear": 35.0},
                    neighbors = [{"rect": (25.0, 60.0, 5.0, 70.0), "height": 32.0}, {"rect": (-35.0, 0.0, 5.0, 70.0), "height": 32.0},
                                 {"rect": (-100.0, 125.0, -120.0, -69.0), "height": 32.0}]),
        floors = [floor(G, name = "Ground floor", z = 0.0, ceiling = 9.0), floor(M, name = "Main floor", z = 10.0, ceiling = 9.0),
                  floor(U, name = "Upper floor", z = 20.0, ceiling = 9.0)],
        rooms = rooms,
        doors = doors,
        windows = windows,
        stacks = stacks,
        courts = courts,
    )


SIDE_HALL_MARKS = [  # the eleven marks, by floor and room (bedroom 2 is not one: see above)
    ("G", "bathG", "its only door is from the garage"),
    ("G", "garage", "No door from the garage into the house"),
    ("G", "mud", "5'-7\" x 19'-7\" mudroom: leftover depth"),
    ("G", "clG1", "3'-6\" x 19'-4\" closet: leftover depth"),
    ("M", "pantry", "a 69'-5\" walk from the kitchen"),
    ("M", "powder", "Powder opens into the dining"),
    ("M", "dining", "of a street window's sky"),
    ("M", "hallM", "41'-6\" hall serves only the living"),
    ("U", "bed3", "7'-5\" wide: the full set does not fit at any depth"),
    ("U", "pbath", "reached only through the primary closet"),
    ("U", "primary", "10'-2\" x 20'-5\": 2.0:1"),
]

def the_side_hall_plan():
    SH = side_hall_plan()
    assert_eq(validate(SH), [])
    # 69'-5": the kitchen's centre, through the dining, the living and the hall, to the pantry's centre
    w = walks(SH, "M", "kitchen", (13.4, 39.577), ["pantry"])["pantry"]
    assert_close(w, 69.377, tol = 0.001)
    return w

test("The side hall rebuilt: it validates, and the kitchen-to-pantry walk is 69'-5\"", the_side_hall_plan)

def the_side_hall_red_pen():
    SH = side_hall_plan()
    A = audit(SH, name = "t_side_hall")
    marks = A["marks"]
    got = {}
    for m in marks:
        if m.get("at") != None:
            got.setdefault((m["floor"], m["room"]), []).append(m["text"])
    for (fl, rk, has) in SIDE_HALL_MARKS:
        texts = got.get((fl, rk), [])
        assert_true(any([has in t for t in texts]), msg = "%s %s: %r not among %s" % (fl, rk, has, texts))
    extra = sorted([k for k in got if k not in [(a, b) for (a, b, _) in SIDE_HALL_MARKS]])
    assert_eq(extra, [("G", "storeG")])
    # bedroom 2 holds the full set with the bed partly in its bay, so no mark
    assert_true(("U", "bed2") not in got, msg = str(got.get(("U", "bed2"))))
    b2 = [r for r in A["rows"] if r["key"] == "furnish:repack:U:bed2"][0]
    assert_true(b2["ok"] and "partly in the room's other rects" in b2["detail"], msg = row_line(b2))
    # the dining's court: 2 % of a street window's sky (0.009 of 0.456 on hand-built boxes; here the
    # plan's own massing, rooms grown to their walls' centre lines)
    d = [r for r in A["rows"] if r["key"] == "light:sky-w5_dining:M:dining"][0]
    assert_true("sees 2% of a street window's sky" in d["detail"], msg = d["detail"])
    # numbered floor by floor, as the sheet reads
    assert_eq([m["floor"] for m in marks], sorted([m["floor"] for m in marks], key = lambda f: {"G": 0, "M": 1, "U": 2}[f]))
    # the ground floor's storage and hall shares, with no place, are in the list with no ring
    assert_true(any([m["of"] == "fillers:shares:G:-" and m.get("at") == None for m in marks]), msg = "the shares are listed")
    return len(marks)

test("The side hall's red pen: eleven marks, none on bedroom 2, and one more", the_side_hall_red_pen)

def furniture_repack():
    SH = side_hall_plan()
    rows = {r["room"]: r for r in furnish_rows(SH, name = "t_fr")}
    assert_eq(sorted(rows.keys()), ["bed2", "bed3", "primary"])
    _expect(rows.values(), "furnish:repack:U:bed3", False, "Bedroom 3, 7'-5\" wide: the full set does not fit at any depth", "fail")
    # with its bay: the bed stands partly in it (plan_furnish packs several rects in their bounding box)
    _expect(rows.values(), "furnish:repack:U:bed2", True, "holds the full set")
    _expect(rows.values(), "furnish:repack:U:bed2", True, "its bed stands partly in the room's other rects")
    _expect(rows.values(), "furnish:repack:U:primary", True, "holds the full set")
    # rooms= picks the rooms; a set named on the room wins over its kind's
    one = furnish_rows(SH, rooms = ["primary"], name = "t_fr1")
    assert_eq([r["room"] for r in one], ["primary"])

test("The furniture re-pack: plan_furnish at the drawn sizes, through room_local()", furniture_repack)

# ================================================================ isovists
def isovists():
    # a 20 x 10 room: from its centre, all 200 sq ft and 11'-2" to the corners
    p = _mk([_R("a", "living", (0.0, 20.0, 0.0, 10.0))])
    v = isovist(p, "F1", (10.0, 5.0), rays = 720)
    assert_close(v["area"], 200.0, tol = 2.0)
    assert_close(v["far"], math.hypot(10.0, 5.0), tol = 0.05)
    # a second room through a 3-ft door: some of it, not all; with the doors shut (cased openings only) none of it
    q = _mk([_R("a", "living", (0.0, 20.0, 0.0, 10.0)), _R("b", "den", (0.0, 20.0, 10.4, 20.4))],
            doors = [_D("d", "b", "a", "S", (10.0, 10.2), width = 3.0)])
    two = isovist(q, "F1", (10.0, 5.0), rays = 720)
    shut = isovist(q, "F1", (10.0, 5.0), rays = 720, doors = "open")
    assert_true(two["area"] > 205.0 and two["area"] < 400.0, msg = str(two))
    assert_close(shut["area"], 200.0, tol = 2.0)
    # the side hall: from the vestibule, and from the stair on each floor
    rows = isovist_rows(side_hall_plan())
    assert_eq(sorted([r["key"] for r in rows]), ["isovist:entry:G:vestibule", "isovist:stair:G:stairG", "isovist:stair:M:stairM", "isovist:stair:U:stairU"])
    for r in rows:
        assert_true(r["ok"] and r["severity"] == "note" and "kept out of every loop" in r["detail"], msg = row_line(r))
    return two["area"]

test("Isovists: openness from the entry and the stair, reported only", isovists)
