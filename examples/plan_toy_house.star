# A plan value drawn as a set: @cadpole/plan.star's synthetic toy house, two floors of a 25 x 60 ft house,
# on one ARCH D sheet -- rooms filled by kind, walls derived from the rooms, doors with their swings, windows (the
# egress ones in blue), stacks, fixtures and a label per room -- with a red pen from a few illustrative checks, and
# a comparison sheet of the house beside a variant that splits the family room to make a guest room.
#
#   cadpole export examples/plan_toy_house.star --run-exports --out-dir /tmp   # /tmp/plan_toy_house.pdf, two ARCH D pages
#   cadpole test examples/plan_toy_house.star
#
# The checks here are illustrations of the check-row format: a row
# with `at` or `rect` is a numbered mark on its floor. The house, its lot and its rules are fictional.

load("@cadpole/plan.star", "area", "door", "ftin", "room", "rooms_on", "sqft", "toy_house", "validate", "window", "windows_of")
load("@cadpole/plan_draw.star", "compare_sheet", "kind_fill", "plan_set", "redpen")
load("@cadpole/building.star", "ft")

units(units.ft_in)

HOUSE = toy_house()

# The plan value drives geometry too: every room a box of its ceiling height on its floor (a deck a 4-in slab), in the
# colour its kind has on the sheets -- the toy house's massing, from the same data the sheets draw.
Z = {f["key"]: f["z"] for f in HOUSE["floors"]}
CEILING = {f["key"]: f["ceiling"] for f in HOUSE["floors"]}
MASSING = []
for r in HOUSE["rooms"]:
    h = 4.0 / 12.0 if r["kind"] == "deck" else (r.get("ceiling") or CEILING[r["floor"]])
    for i, x in enumerate(r["rects"]):
        b = box(ft(x[1] - x[0]), ft(x[3] - x[2]), ft(h), at = (ft(x[0]), ft(x[2]), ft(Z[r["floor"]])), name = "plan_toy_%s_%d" % (r["key"], i))
        b.material(color = kind_fill(r["kind"]))
        MASSING.append(b)

def variant():
    """The family room split: a guest room at its east end, off the kitchen, with a side window that is too small to
    escape through."""
    rooms = []
    for r in HOUSE["rooms"]:
        if r["key"] == "family":
            rooms.append(room("family", "Family", floor = "L1", kind = "family", rect = (0.5, 14.0, 50.4, 69.5)))
            rooms.append(room("guest", "Guest", floor = "L1", kind = "guest", rect = (14.4, 24.5, 50.4, 69.5)))
        else:
            rooms.append(r)
    doors = [d for d in HOUSE["doors"] if d["key"] != "d_rear"] + [
        door("d_rear", floor = "L1", a = "family", b = "outside:yard", wall = "N", at = (7.25, 69.5), width = 6.0, swing = "slide"),
        door("d_guest", floor = "L1", a = "guest", b = "kitchen", wall = "S", at = (19.5, 50.4), width = 2.67, swing = "a", hinge = "lo"),
    ]
    windows = list(HOUSE["windows"]) + [
        window("w_guest", floor = "L1", room = "guest", wall = "E", at = (24.5, 60.0), width = 2.0, sill = 4.0, head = 7.0, source = "side"),
        window("w_guest_n", floor = "L1", room = "guest", wall = "N", at = (19.5, 69.5), width = 3.0, sill = 3.5, head = 7.0, source = "rear"),
    ]
    return dict(HOUSE, name = "toy_house_guest", rooms = rooms, doors = doors, windows = windows, source = "examples/plan_toy_house.star")

VARIANT = variant()

def checks(p):
    """Illustrative rows: halls 3'-0" wide (R311.6), habitable rooms 70 sq ft and 7'-0" both ways (R304), an escape
    window for every sleeping room (R310), and a house rule of 2'-8" doors except into closets."""
    rows = []
    for fl in [f["key"] for f in p["floors"]]:
        for r in rooms_on(p, fl):
            m = r["rects"][0]
            w, d = m[1] - m[0], m[3] - m[2]
            if r["kind"] == "hall":
                rows.append({"key": "demo:%s:%s:hall" % (fl, r["key"]), "name": "Hall width", "ok": min(w, d) >= 3.0,
                             "detail": "%s clear" % ftin(min(w, d)), "rule": "R311.6", "severity": "fail", "floor": fl, "room": r["key"], "rect": m})
            if r.get("habitable") and r["kind"] not in ["kitchen"]:
                ok = area(r) >= 70.0 and min(w, d) >= 7.0
                rows.append({"key": "demo:%s:%s:size" % (fl, r["key"]), "name": "Room size", "ok": ok,
                             "detail": "%s, %s x %s" % (sqft(area(r)), ftin(w), ftin(d)), "rule": "R304", "severity": "fail", "floor": fl, "room": r["key"], "rect": m})
            if r.get("sleeping"):
                ws = [x for x in windows_of(p, r["key"]) if x.get("egress")]
                at = None if ws else ((m[0] + m[1]) / 2.0, (m[2] + m[3]) / 2.0)
                rows.append({"key": "demo:%s:%s:egress" % (fl, r["key"]), "name": "Escape window", "ok": len(ws) > 0,
                             "detail": "none of its windows is an escape window" if not ws else ws[0]["key"], "rule": "R310", "severity": "fail", "floor": fl, "room": r["key"], "at": at})
    by = {r["key"]: r for r in p["rooms"]}
    for d in p["doors"]:
        if d.get("swing") == "open" or by.get(d["a"], {}).get("kind") == "closet":
            continue
        rows.append({"key": "demo:%s:%s:width" % (d["floor"], d["key"]), "name": "Door width", "ok": d["width"] >= 2.66,
                     "detail": "%s clear" % ftin(d["width"]), "rule": "house rule", "severity": "warn", "floor": d["floor"], "room": d["a"], "at": d["at"]})
    return rows

CHECKS = checks(HOUSE)
VCHECKS = checks(VARIANT)

S = plan_set(HOUSE, name = "plan_toy_house", checks = CHECKS, project = "A TOY HOUSE", stamp = ["PLAN STUDY", "SYNTHETIC"])
compare_sheet(S, [HOUSE, VARIANT], checks = [CHECKS, VCHECKS], labels = ["As drawn", "With a guest room"])
export(S, "plan_toy_house.pdf")

def both_plans_are_clean():
    assert_eq(validate(HOUSE), [])
    assert_eq(validate(VARIANT), [])
    return len(VARIANT["rooms"])

test("the house and its variant validate clean", both_plans_are_clean)

def the_red_pen_catches_the_guest_room():
    fails = [r["key"] for _, r in redpen(VCHECKS)]
    assert_true("demo:L1:guest:egress" in fails, msg = str(fails))
    assert_true("demo:L1:guest:egress" not in [r["key"] for _, r in redpen(CHECKS)])
    assert_eq(len([1 for _, r in redpen(CHECKS) if r["rule"] == "house rule"]), 3)  # the powder, bath and laundry doors
    return len(fails)

test("the variant's guest room has no escape window, and the red pen says so", the_red_pen_catches_the_guest_room)

def the_massing_follows_the_plan():
    assert_eq(len(MASSING), 17)  # 16 rooms, the kitchen an L of two rects
    for b in MASSING:
        assert_solid(b)
    return len(MASSING)

test("every room a box on its floor, from the plan value", the_massing_follows_the_plan)

def the_set_has_its_sheets():
    assert_eq(S.sheets(), ["A1.1", "C1.0"])
    return len(S.sheets())

test("one plan sheet for both floors, and the comparison", the_set_has_its_sheets)
