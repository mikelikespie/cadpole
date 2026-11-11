# Tests for @cadpole/plan_draw.star: a floor laid out without a sheet (every room labelled once, no two labels
# on top of each other, labels inside their rooms unless set outside with a leader), the red pen's numbering and
# marks, the architect's scale that fits, and the sheet set and the comparison sheet built from the module's
# synthetic toy_house(). `cadpole test stdlib/plan_draw_test.star`.

load("@cadpole/plan.star", "get_room", "room", "toy_house")
load("@cadpole/plan_draw.star", "compare_sheet", "floor_layout", "kind_fill", "plan_set", "redpen", "scale_for", "scorecard")

T = toy_house()

ROWS = [
    {"key": "demo:ok", "name": "A rule that holds", "ok": True, "detail": "", "rule": "demo", "severity": "fail", "floor": "L1", "room": "living"},
    {"key": "demo:L2:hall2", "name": "Hall width", "ok": False, "detail": "2'-6\" clear", "rule": "R311.6", "severity": "fail", "floor": "L2", "room": "hall2", "rect": (12.4, 19.6, 18.4, 40.0)},
    {"key": "demo:L1:powder", "name": "Powder door", "ok": False, "detail": "swings over the WC", "rule": "house", "severity": "warn", "floor": "L1", "room": "powder", "at": (20.0, 33.2)},
    {"key": "demo:note", "name": "A note", "ok": False, "detail": "", "rule": "demo", "severity": "note", "floor": "L1", "at": (5.0, 15.0)},
    {"key": "demo:L1:kitchen", "name": "Kitchen landing", "ok": False, "detail": "short of 15 in", "rule": "NKBA", "severity": "fail", "floor": "L1", "room": "kitchen"},
]

def _disjoint(boxes):
    for i in range(len(boxes)):
        for j in range(i + 1, len(boxes)):
            a, b = boxes[i], boxes[j]
            if a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]:
                return (i, j)
    return None

def every_room_gets_one_label():
    for fl in ["L1", "L2"]:
        lay = floor_layout(T, fl)
        keys = [r["key"] for r in T["rooms"] if r["floor"] == fl]
        assert_eq(sorted(lay["labels"].keys()), sorted(keys), msg = fl)
        assert_eq(lay["overlaps"], [])
        boxes = [v["box"] for v in lay["labels"].values()]
        assert_eq(_disjoint(boxes), None, msg = "two labels overlap on " + fl)
        for k, v in lay["labels"].items():
            assert_true(v["how"] in ["inside", "shrunk", "upright", "outside"])
    big = floor_layout(T, "L1")["labels"]
    assert_eq(big["kitchen"]["how"], "inside")
    assert_eq(big["family"]["lines"], 3)  # name, sq ft, W x D
    return len(big)

test("every room gets one label, clear of every other", every_room_gets_one_label)

def labels_shrink_then_go_outside():
    # at 1/16" = 1'-0" the closets and the powder room cannot hold their three lines
    lay = floor_layout(T, "L1", scale = 0.0625 * 25.4)
    hows = {k: v["how"] for k, v in lay["labels"].items()}
    assert_true("outside" in hows.values() or "shrunk" in hows.values() or "upright" in hows.values(), msg = str(hows))
    assert_eq(_disjoint([v["box"] for v in lay["labels"].values()]), None)
    assert_true(lay["w"] > floor_layout(T, "L1", scale = 0.0625 * 25.4, labels = False)["w"] - 1e-9)
    return len([h for h in hows.values() if h != "inside"])

test("labels shrink, turn upright or go outside with a leader when a room is small", labels_shrink_then_go_outside)

def the_red_pen_numbers_failing_rows():
    rp = redpen(ROWS)
    assert_eq([n for n, _ in rp], [1, 2, 3])
    assert_eq([r["key"] for _, r in rp], ["demo:L2:hall2", "demo:L1:powder", "demo:L1:kitchen"])
    assert_eq(len(redpen(ROWS, notes = True)), 4)
    l1 = floor_layout(T, "L1", checks = ROWS)
    assert_eq([n for n, _, _ in l1["marks"]], [2, 3])
    assert_eq([n for n, _ in l1["rows"]], [2, 3])
    l2 = floor_layout(T, "L2", checks = ROWS)
    assert_eq([n for n, _, _ in l2["marks"]], [1])
    # a mark sits where its row says, on paper: the powder door's centre
    lay = floor_layout(T, "L1", checks = ROWS)
    m = [c for n, _, c in lay["marks"] if n == 2][0]
    ml, mb = lay["margin"]
    x0, y0 = lay["origin"]
    assert_close(m, (ml + (20.0 - x0) * lay["s"], mb + (33.2 - y0) * lay["s"]), tol = 1e-9)
    # the labels keep off the marks
    boxes = [v["box"] for v in lay["labels"].values()]
    for _, _, c in lay["marks"]:
        for b in boxes:
            assert_true(not (b[0] < c[0] and c[0] < b[2] and b[1] < c[1] and c[1] < b[3]), msg = "a label covers a mark")
    return len(rp)

test("the red pen numbers the failing rows and marks them on their floors", the_red_pen_numbers_failing_rows)

def scales_and_fills():
    assert_eq(scale_for(25.0, 60.0, 400.0, 500.0)[1], "1/4\" = 1'-0\"")
    assert_eq(scale_for(25.0, 60.0, 600.0, 700.0, largest = 1.0)[1], "3/8\" = 1'-0\"")
    assert_eq(scale_for(25.0, 60.0, 100.0, 100.0)[1], "1/16\" = 1'-0\"")
    assert_eq(kind_fill("bed"), "#e6dff0")
    assert_eq(kind_fill("nonsense"), kind_fill("other"))
    return 0

test("the architect's scale that fits, and the fills by kind", scales_and_fills)

def sheets_build():
    S = plan_set(T, checks = ROWS)
    assert_eq(S.sheets(), ["A1.1"])  # both floors fit one ARCH D sheet side by side
    v = dict(T, name = "toy_variant", rooms = [r if r["key"] != "bed2" else room("bed2", "Bedroom 2", floor = "L2", kind = "bed", rect = (0.5, 10.0, 24.4, 40.0)) for r in T["rooms"]])
    sh = compare_sheet(S, [T, v], checks = [ROWS, None])
    assert_eq(sh.number(), "C1.0")
    assert_eq(S.sheets(), ["A1.1", "C1.0"])
    card = {k: x for k, x in scorecard(T, ROWS)}
    assert_eq(card["rooms"], "15")
    assert_eq(card["bedrooms"], "3")
    assert_eq(card["failing checks"], "3")
    assert_eq(card["plan problems"], "0")
    return len(S.sheets())

test("a plan set and a comparison sheet build from a plan value", sheets_build)
