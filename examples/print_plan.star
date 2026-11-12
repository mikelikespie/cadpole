# print_plan.star — a small trinket box, modelled the way it is used, and
# plated for printing by `print_plan`: nothing here is turned or moved by
# hand for the printer.
#
#   * the box and its lid, the lid sitting on the box: the lid lifts off,
#     so it is printed apart, and turned over onto its top;
#   * a wheel on an axle between two caps, 0.35 mm clear: captive, so it is
#     printed in place, as modelled;
#   * a latch whose arm bends: `along=` keeps the arm along the layers;
#   * a two-colour badge: a stripe lying on a plate is one piece;
#   * a test coupon of the latch, printed first on a plate of its own.
#
#   cadpole eval examples/print_plan.star        # the plan's report is an info diagnostic
#   cadpole export examples/print_plan.star --run-exports --out-dir /tmp/plan
#   cadpole plate examples/print_plan.star --printer bambu_h2c -o /tmp/plan.3mf

printer_name = param("printer", "bambu_h2d", choices=["bambu_h2d", "bambu_h2c", "bambu_x1c", "bambu_a1"], doc="The printer the plates are laid out for")
mono = param("mono", False, doc="Also make a set of plates all in one filament")

W, D, H, WALL = 70.0, 50.0, 30.0, 2.0
GAP = 0.35  # the wheel's clearance on every face

# --- The box and its lid, as they sit.
box_body = subtract(box(W, D, H), box(W - 2 * WALL, D - 2 * WALL, H, at=(WALL, WALL, WALL)), name="box_body")
lid = subtract(box(W, D, 8.0, at=(0, 0, H)), box(W - 2 * WALL, D - 2 * WALL, 6.0, at=(WALL, WALL, H)), name="lid")
box_body.material(color="#2F6F8F")
lid.material(color="#F4F3EE")

# --- A wheel held on its axle by a cap at each end.
axle = union(
    cylinder(d=4.0, height=14.0, at=(W + 30, 0, 0)),
    cylinder(d=12.0, height=2.0, at=(W + 30, 0, 0)),
    cylinder(d=12.0, height=2.0, at=(W + 30, 0, 12.0)),
    name="axle",
)
wheel = subtract(
    cylinder(d=24.0, height=10.0 - 2 * GAP, at=(W + 30, 0, 2.0 + GAP)),
    cylinder(d=4.0 + 2 * GAP, height=12.0, at=(W + 30, 0, 1.0)),
    name="wheel",
)
axle.material(color="#2F6F8F")
wheel.material(color="#161616")

# --- A latch: a 2 mm arm 24 mm long on a block, the arm standing up as
# drawn. Printed standing, it would bend across its layers.
latch = union(box(12.0, 8.0, 6.0, at=(0, D + 20, 0)), box(2.0, 8.0, 24.5, at=(10.0, D + 20, 5.5)), name="latch")
latch.material(color="#2F6F8F")
coupon = translate(copy(latch), (40.0, 0, 0), name="latch_coupon")

# --- A badge with a stripe lying on it.
badge = box(40.0, 20.0, 3.0, at=(0, -60.0, 0), name="badge")
stripe = box(8.0, 20.0, 0.6, at=(16.0, -60.0, 3.0), name="stripe")
badge.material(color="#2F6F8F")
stripe.material(color="#FFCE1F")

plan = print_plan(
    [
        box_body,
        lid,
        axle,
        wheel,
        print_part(latch, along="+z", name="latch"),
        print_part(coupon, along="+z", coupon=True, name="latch coupon"),
        badge,
        stripe,
    ],
    "print_plan.3mf",
    printer=printer_name,
    variants=["colour", "mono"] if mono else ["colour"],
)
plate_images("print_plan.3mf")


def piece(name):
    return [p for p in plan["pieces"] if p["name"] == name][0]


def the_lid_prints_on_its_top():
    p = piece("lid")
    assert_true(p["support_area"] == 0.0, msg=p["reason"])
    assert_true(p["modelled_support_area"] > 1000.0, msg=p["reason"])


def the_wheel_prints_on_its_axle():
    p = [p for p in plan["pieces"] if p["kind"] == "print-in-place"]
    assert_eq(len(p), 1)
    assert_eq(p[0]["orientation"], "as modelled")


def the_latch_arm_runs_along_the_layers():
    p = piece("latch")
    assert_true(p["orientation"] != "as modelled", msg=p["reason"])


def the_coupon_comes_first():
    assert_eq(plan["plates"][0]["name"], "Test coupons")
    assert_eq(plan["plates"][0]["pieces"], ["latch coupon"])


def the_badge_and_its_stripe_are_one_piece():
    p = [p for p in plan["pieces"] if p["kind"] == "fused"]
    assert_eq(len(p), 1)
    assert_eq(len(p[0]["bodies"]), 2)


test("the lid prints on its top", the_lid_prints_on_its_top)
test("the wheel prints on its axle", the_wheel_prints_on_its_axle)
test("the latch arm runs along the layers", the_latch_arm_runs_along_the_layers)
test("the coupon comes first", the_coupon_comes_first)
test("the badge and its stripe are one piece", the_badge_and_its_stripe_are_one_piece)
