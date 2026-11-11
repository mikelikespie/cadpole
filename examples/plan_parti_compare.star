# Four partis, one program: four 25-ft row-house partis -- side hall, cross stair, middle
# court, open core -- each fitted by @cadpole/plan_parti.star in one MILP over three floors and proven optimal, on one
# comparison sheet (every floor of each, its scorecard and its red pen), and as blocks in the viewer.
#
#   cadpole export examples/plan_parti_compare.star --run-exports --out-dir /tmp    # /tmp/plan_parti_compare.pdf
#   cadpole test examples/plan_parti_compare.star
#
# The fits are stored plan values (examples/plan_parti_fits.star), so the sheet draws without solving. This file is
# also their generator: `-p refit=<parti>` (or <parti>:<knob>) fits one parti, proven optimal (5 to 60 s), and prints
# its line of the fits file. To regenerate it, one parti per run inside the CLI's 120 s:
#
#   K=bin/cadpole; F=$(mktemp)
#   ( sed -n '1,6p' examples/plan_parti_fits.star
#     for p in side_hall cross_stair court open_core side_hall:primary_at_street; do
#       $K eval examples/plan_parti_compare.star -p refit=$p | sed -n 's/^info: print: FIT //p'
#     done
#     tail -n 1 examples/plan_parti_fits.star ) > $F && mv $F examples/plan_parti_fits.star
#
# partis_sheet() draws the sheet with @cadpole/plan_draw.star's compare_sheet; the red pen is @cadpole/plan_audit.star's
# audit (doors, stacks, fillers, code rows) and the vetted rules' checks (@cadpole/plan_rules.star's standing set).
# "Dark" is the audit's light measure (court windows lit as deep as view_cone's sky table says), beside the fit's own
# figure, which assumes a court lights 12 ft deep. Both are uncalibrated. The lot and its houses are synthetic.

load("@cadpole/plan.star", "validate")
load("@cadpole/plan_parti.star", "compare", "fit", "partis_sheet", "slim")
load("@cadpole/plan_templates.star", "LIBRARY", "knob")
load("plan_parti_fits.star", "FITS", "MASSING")

units(units.ft_in)

refit = param("refit", "")  # "<parti>" or "<parti>:<knob>": fit it, proven optimal, and print its line of the fits file
if refit != "":
    parts = refit.split(":")
    T = LIBRARY[parts[0]]()
    if len(parts) > 1:
        T = knob(T, parts[1])
    print("FIT FITS[%r] = %s" % (T["key"], slim(fit(T, name = "plan_parti_refit_" + T["key"]))))

ORDER = ["side_hall", "cross_stair", "court", "open_core"]
PLANS = [FITS[k] for k in ORDER]

S = sheet_set(
    "plan_parti_compare",
    size = sheet.arch_d,
    units = "ft-in",
    title_block = title_block(project = "FOUR PARTIS, ONE PROGRAM", stamp = ["PLAN STUDY", "SYNTHETIC LOT"],
                              notes = "From @cadpole/plan_parti.star: one MILP per parti over three floors, proven optimal."),
)
OUT = partis_sheet(S, PLANS, labels = ["1  Side hall", "2  Cross stair", "3  Middle court", "4  Open core"])
export(S, "plan_parti_compare.pdf")

def the_fits_are_the_proven_optima():
    # the proven optima, as a reference solve on HiGHS gives them
    want = {"side_hall": 1091.926, "cross_stair": 1048.656, "court": 1155.950, "open_core": 1247.268}
    for k in ORDER:
        c = FITS[k]["meta"]["plan_parti"]
        assert_eq(c["status"], "optimal", msg = k)
        assert_close(c["objective"], want[k], tol = 0.01, msg = k)
    return len(ORDER)

test("the stored fits are the proven optima", the_fits_are_the_proven_optima)

def the_partis_differ_in_organization():
    # 12 to 16 of the 16 program rooms differ in character between partis
    got = {(x["a"], x["b"]): x["differ"] for x in OUT["compare"]["pairs"]}
    assert_eq(got[("side_hall", "cross_stair")], 16)
    assert_eq(got[("side_hall", "court")], 15)
    assert_eq(got[("side_hall", "open_core")], 12)
    assert_eq(got[("cross_stair", "court")], 14)
    assert_eq(got[("cross_stair", "open_core")], 14)
    assert_eq(got[("court", "open_core")], 14)
    return min(got.values())

test("between partis, 12 to 16 of the 16 program rooms differ in character", the_partis_differ_in_organization)

def the_plans_are_clean_and_the_red_pen_finds_the_known_faults():
    for p in PLANS:
        assert_eq(validate(p), [])
    keys = [r.get("of") or r["key"] for r in OUT["checks"][0]]
    for k in ("doors:garage-door:G:garage", "doors:bath-reach:G:bathG", "doors:pantry:M:pantry", "storage_cap:mud"):
        assert_true(k in keys, msg = "%s in the side hall's red pen: %s" % (k, keys))
    # measured, the side hall's court-lit dining room is dark: its 5'-7" rear arm lights the main floor 0 ft deep
    sh = FITS["side_hall"]["meta"]["plan_parti"]
    assert_true(sh["light"]["dark_sqft"] > sh["metrics"]["dark_sqft_assumed"] + 50.0)
    return len(keys)

test("the plans validate, and the red pen finds the known faults", the_plans_are_clean_and_the_red_pen_finds_the_known_faults)

def the_houses_stand_as_blocks():
    # every indoor room of the four fits a block, decks as slabs, courts open
    assert_eq([len(m) for m in MASSING], [31, 34, 28, 31])
    return len(MASSING)

test("the four houses stand as blocks in the viewer", the_houses_stand_as_blocks)

def the_knob_moves_the_primary_to_the_street():
    # the side hall with the primary bedroom at the street: the knob's re-fit scores
    # 11.4 more and changes 6 of 16 rooms
    a, b = FITS["side_hall"], FITS["side_hall_front_master"]
    ca, cb = a["meta"]["plan_parti"], b["meta"]["plan_parti"]
    assert_close(cb["objective"] - ca["objective"], 11.37, tol = 0.05)
    pair = compare([a, b])["pairs"][0]
    assert_eq(pair["differ"], 6)
    return pair["differ"]

test("a knob re-fit: the side hall's primary bedroom at the street", the_knob_moves_the_primary_to_the_street)
