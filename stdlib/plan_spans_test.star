# The tests of @cadpole/plan_spans.star: the code tables read back as the code prints them, the interpolation
# their footnotes allow, and the rules of thumb behaving like rules (monotone, conservative, None past their end).
# `cadpole test stdlib/plan_spans_test.star`.

load("@cadpole/plan_spans.star", "FLOOR_30", "FLOOR_40", "HEADERS_EXT", "HEADERS_INT", "RAFTER_20", "ftin",
     "header_for", "header_span", "ijoist_span", "joist_options", "joist_span", "lvl_beam", "rafter_span", "span_ft",
     "steel_beam")

def close(a, b, tol=1e-9):
    assert_close(a, b, tol=tol)

def tables_are_whole():
    # Four spacings by four species by four grades, in each joist table; every row has every size at both dead loads.
    for t in (FLOOR_40, FLOOR_30, RAFTER_20):
        assert_eq(len(t["rows"]), 64, msg=t["table"])
        for r in t["rows"]:
            assert_eq(len(r[3].split(" ")), len(t["sizes"]), msg="%s %s" % (t["table"], r))
            assert_eq(len(r[4].split(" ")), len(t["sizes"]), msg="%s %s" % (t["table"], r))
    for sup, rows in HEADERS_EXT["supporting"].items():
        assert_eq(len(rows), 15, msg=sup)
        for r in rows:
            assert_eq(len(r[1].split(" ")), 18, msg="%s %s" % (sup, r[0]))  # (span, NJ) x 3 widths x 3 snow loads
    for sup, rows in HEADERS_INT["supporting"].items():
        assert_eq(len(rows), 11, msg=sup)
        for r in rows:
            assert_eq(len(r[1].split(" ")), 6, msg="%s %s" % (sup, r[0]))
    return 64 * 3

test("every table has every row and column the code prints", tables_are_whole)

def the_dead_load_column_matters():
    # A DF-L No. 2 2x12 at 16 in spans 16'-6" at 20 psf dead, not 18'-1" (that is the 10-psf column).
    close(joist_span("2x12", 16), 16.5)
    close(joist_span("2x12", 16, dead=10), 18 + 1 / 12.0)
    assert_eq(ftin(joist_span("2x12", 16)), "16'-6\"")
    assert_eq(ftin(joist_span("2x12", 16, dead=10)), "18'-1\"")
    return ftin(joist_span("2x12", 16))

test("a 2x12 at 16 in spans 16'-6\" at 20 psf dead, 18'-1\" at 10", the_dead_load_column_matters)

def sacramento_handout_matches():
    # City of Sacramento CDD-0277 (rev. 2023-01-01) prints 2022 CRC Table R502.3.1(2) for DF-L No. 2: 10 psf | 20 psf.
    want = {
        ("2x6", 12): ("10-9", "10-8"), ("2x6", 16): ("9-9", "9-3"), ("2x6", 24): ("8-3", "7-6"),
        ("2x8", 12): ("14-2", "13-6"), ("2x8", 16): ("12-9", "11-8"), ("2x8", 24): ("10-5", "9-6"),
        ("2x10", 12): ("18-0", "16-5"), ("2x10", 16): ("15-7", "14-3"), ("2x10", 24): ("12-9", "11-8"),
        ("2x12", 12): ("20-11", "19-1"), ("2x12", 16): ("18-1", "16-6"), ("2x12", 24): ("14-9", "13-6"),
    }
    for (size, sp), (a, b) in want.items():
        close(joist_span(size, sp, dead=10), span_ft(a))
        close(joist_span(size, sp, dead=20), span_ft(b))
    # and its rafters, R802.4.1(2): 2x10 at 16 in, 22'-6" | 19'-6"; 2x12 at 16 in, 26'-0" | 22'-7".
    close(rafter_span("2x10", 16, dead=10), 22.5)
    close(rafter_span("2x10", 16), 19.5)
    close(rafter_span("2x12", 16, dead=10), 26.0)
    close(rafter_span("2x12", 16), 22 + 7 / 12.0)
    return len(want)

test("the DF-L No. 2 rows match Sacramento's handout of the same tables", sacramento_handout_matches)

def other_rows_read_as_printed():
    close(joist_span("2x10", 19.2, species="HF", grade="#1"), 13.5)            # 13-6
    close(joist_span("2x12", 24, species="HF", grade="SS"), 16 + 10 / 12.0)    # 16-10a (end bearing 2 in)
    close(joist_span("2x12", 16, live=30), 18 + 1 / 12.0)                      # R502.3.1(1), sleeping areas
    close(joist_span("2x8", 12, species="SPF", grade="#3", dead=10), 11.0)     # 11-0
    close(rafter_span("2x12", 12, dead=10), 26.0)                              # note b: over 26 ft
    return 5

test("rows of other species, grades and tables read as printed", other_rows_read_as_printed)

def headers_interpolate_as_footnote_c_allows():
    h = header_span("2-2x10", "interior", "One floor only", 24)
    close(h["span"], 6.5)
    assert_eq(h["jacks"], 2)
    close(header_span("2-2x10", "interior", "One floor only", 30)["span"], 6.5 + 0.5 * (5.25 - 6.5))
    close(header_span("2-2x10", "interior", "One floor only", 8)["span"], 9 + 2 / 12.0)     # under 12 ft: the 12-ft column
    assert_eq(header_span("2-2x10", "interior", "One floor only", 40), None)               # past 36 ft: no answer
    e = header_span("2-2x8", "exterior", "Roof, ceiling and one center bearing floor", 24)
    close(e["span"], 4 + 10 / 12.0)
    assert_eq(e["jacks"], 2)
    close(header_span("2-2x8", "exterior", "Roof, ceiling and one center bearing floor", 24, snow=70)["span"], 4 + 1 / 12.0)
    close(header_span("2-2x10", "interior", "Two floors", 12, braced=False)["span"], (5 + 11 / 12.0) * 0.70)
    return h["span"]

test("R602.7 spans interpolate by building width, and stop past 36 ft", headers_interpolate_as_footnote_c_allows)

def the_least_header_that_spans():
    h = header_for(6.0, "interior", "One floor only", 24)
    assert_eq(h["size"], "2-2x10")    # 6'-6"; 3-2x8 (6'-10") and 2-2x12 (7'-7") hold more lumber
    assert_eq(header_for(3.0, "exterior", "Roof and ceiling", 24)["size"], "2-2x4")
    assert_eq(header_for(16.0, "interior", "One floor only", 24), None)   # nothing in R602.7(2) reaches 16 ft
    return h["size"]

test("header_for picks the least lumber whose span covers the opening", the_least_header_that_spans)

def rules_of_thumb_behave():
    # I-joists: deeper spans further, wider spacing less far; the roof rule longer, the deck rule shorter.
    a, b = ijoist_span(11.875, 16), ijoist_span(14.0, 16)
    assert_true(b > a)
    assert_true(ijoist_span(11.875, 24) < a)
    assert_true(ijoist_span(11.875, 16, use="roof") > a)
    assert_true(a > ijoist_span(11.875, 16, use="deck"))
    close(a, 11.875 * 1.55)
    # Beams: a heavier load never takes a smaller beam; past four plies of the deepest LVL there is no answer.
    l1, l2 = lvl_beam(12.0, 400.0), lvl_beam(12.0, 1200.0)
    assert_true(l1["util"] <= 1.0 and l2["util"] <= 1.0)
    assert_true(l2["b"] * l2["depth"] >= l1["b"] * l1["depth"])
    assert_eq(lvl_beam(40.0, 3000.0), None)
    s1, s2 = steel_beam(20.0, 1500.0), steel_beam(20.0, 4000.0)
    assert_true(s2["weight"] >= s1["weight"])
    return [l1["size"], l2["size"], s1["size"], s2["size"]]

test("the rules of thumb are monotone and stop where they should", rules_of_thumb_behave)

def options_for_a_menu():
    o = joist_options()
    assert_eq(len(o), 3 * 4 + 4 * 4)   # DF-L No. 2 2x8, 2x10, 2x12 at four spacings; four I-joist depths at four
    for x in o:
        assert_true(x["spans"]["deck"] < x["spans"]["floor"])
        assert_true(x["spans"]["floor"] <= x["spans"]["roof"])
    return len(o)

test("joist_options lists every joist a default menu may use", options_for_a_menu)

def feet_and_inches():
    assert_eq(ftin(16.5), "16'-6\"")
    assert_eq(ftin(0.25), "0'-3\"")
    assert_eq(ftin(3 + 3.5 / 12.0, 2), "3'-3 1/2\"")
    assert_eq(ftin(-1.0), "-1'-0\"")
    assert_eq(ftin(11.99), "12'-0\"")
    return ftin(16.5)

test("ftin writes feet and inches", feet_and_inches)
