# A house printed one story at a time: a small two-story house drawn
# in feet out of boxes, the way a massing model usually is, cut into story
# pieces with no ceilings -- the next story's floor plate is the ceiling, and
# the roof is a plate of its own -- each joined into one solid, pegged to the
# next, scaled to 1:96 and exported as a Bambu Studio project with a plate
# per piece.
#
#   cadpole test examples/story_kit.star                        # the checks below
#   cadpole check examples/story_kit.star --layer kit           # the pieces, and nothing of the house they came from
#   cadpole check examples/story_kit.star --layer kit --max-bridge 40
#   cadpole export examples/story_kit.star --run-exports --out-dir /tmp   # /tmp/story_kit.3mf
#
# What it shows:
#
#   * **selecting by layer**: the print takes the walls and floors and
#     leaves the glass (too thin to print) and the furniture out, with
#     `bodies(layer=, visible=)`;
#   * **joining boxes that only touch**: a porch post meets the slab corner
#     along an edge, and a beam meets a wall at a corner -- `union(weld=)`
#     bridges both, where a plain `union` refuses the post;
#   * **bridges**: the 3-ft window head is a 9.5 mm span at 1:96, which
#     `print_check` counts as a bridge at the default `max_bridge` (10 mm);
#     the 6-ft one is 19 mm, an overhang until `max_bridge=20`;
#   * **pegs**: three square pegs on the ground floor's partition tops, and
#     their sockets in the upper floor, sized for 0.5-ft walls at 1:96.

load("@cadpole/print.star", "PEG", "is_box", "story_bands")

S = 304.8 / 96.0                     # 1/8" = 1'-0": millimetres per foot
W, D = 24.0, 16.0                    # the house's plan, ft
T = 0.5                              # an exterior wall, ft
P = 0.4                              # a partition, ft

walls = layer("walls", doc = "Walls, posts and beams")
floors = layer("floors", doc = "Slabs and floor plates")
glass = layer("glass", doc = "Glazing: too thin to print at 1:96", opacity = 0.4)
furniture = layer("furniture", doc = "Furniture, not printed")

def story(z, h, tag):
    """Four walls `h` tall from `z`, with a 3-ft and a 6-ft window in the south
    wall (sill at 3, head at 7), and a partition across the middle."""
    out = []
    # The south wall as piers, sills and heads round its two windows.
    piers = [(0.0, 4.0), (7.0, 13.0), (19.0, W)]
    for i, (x0, x1) in enumerate(piers):
        out.append(box(x1 - x0, T, h, at = (x0, 0, z), name = "%s_s_pier%d" % (tag, i), layer = walls))
    for i, (x0, x1) in enumerate([(4.0, 7.0), (13.0, 19.0)]):
        out.append(box(x1 - x0, T, 3, at = (x0, 0, z), name = "%s_s_sill%d" % (tag, i), layer = walls))
        out.append(box(x1 - x0, T, h - 7, at = (x0, 0, z + 7), name = "%s_s_head%d" % (tag, i), layer = walls))
        out.append(box(x1 - x0, 0.08, 4, at = (x0, T / 2, z + 3), name = "%s_glass%d" % (tag, i), layer = glass))
    out.append(box(W, T, h, at = (0, D - T, z), name = tag + "_n", layer = walls))
    out.append(box(T, D - 2 * T, h, at = (0, T, z), name = tag + "_w", layer = walls))
    out.append(box(T, D - 2 * T, h, at = (W - T, T, z), name = tag + "_e", layer = walls))
    out.append(box(P, D - 2 * T, h, at = (12, T, z), name = tag + "_part", layer = walls))
    out.append(box(4, 2, 2.5, at = (2, 8, z), name = tag + "_table", layer = furniture))
    return out

house = (
    [box(W, D, 0.5, at = (0, 0, -0.5), name = "slab", layer = floors)] +
    story(0, 9.5, "l0") +
    [box(W, D, 0.5, at = (0, 0, 9.5), name = "l1_floor", layer = floors)] +
    story(10, 9.0, "l1") +
    [box(W, D, 0.5, at = (0, 0, 19), name = "roof", layer = floors)] +
    # A porch post that meets the slab's corner only along an edge, and a
    # beam that meets the east wall's top only at a corner.
    [box(0.5, 0.5, 9, at = (-0.5, -0.5, -0.5), name = "porch_post", layer = walls),
     box(2, 0.5, 0.5, at = (W, D, 9.0), name = "beam", layer = walls)]
)

# ----------------------------------------------------------------- the print

kit = layer("kit", doc = "The print: one piece per story, at 1:96")
parts = [b for b in bodies(layer = [walls, floors], visible = True) if is_box(b)]
PEGS = {"size": 1.2, "height": 1.0, "chamfer": 0.2}    # 0.5-ft walls are 1.6 mm at 1:96
# Cut at the underside of every floor plate: the ground floor with its slab,
# the upper floor with its floor plate, and the roof plate.
pieces = story_bands(parts, [-0.5, 9.5, 19.0, 19.5], scale = S, weld = 0.01, pegs_per_joint = 3, peg = PEGS,
                     layer = kit, name = "kit")
for label, piece in pieces:
    export(piece, "story_kit.3mf", flavor = flavor.bambu, plate = label, name = label, extruder = 1)

# ----------------------------------------------------------------- the tests

def every_piece_is_one_solid_on_the_bed():
    assert_eq([label for label, _ in pieces], ["main.0", "main.1", "main.2"])
    for label, piece in pieces:
        r = print_check(piece, bed = (350, 320, 325))
        assert_true(r.solid, msg = label + ": " + r.summary)
        assert_true(r.fits_bed, msg = label + " fits an H2D")
        lo, hi = piece.bbox()
        assert_close(lo[2], 0, tol = 1e-4, msg = label + " stands on the bed")
        assert_eq(piece.layer(), "kit")

def the_selection_leaves_glass_and_furniture_out():
    names = [b.name for b in parts]
    assert_true("l0_glass0" not in names and "l0_table" not in names)
    assert_eq(len(bodies(layer = glass)), 4)
    assert_true("porch_post" in names and "beam" in names)

def the_three_foot_head_is_a_bridge_and_the_six_foot_one_is_not():
    lower = pieces[0][1]
    r = print_check(lower)
    assert_true(r.bridges >= 1, msg = "the 3-ft head bridges: " + r.summary)
    assert_true(r.longest_bridge < 10, msg = "a 9.5 mm span")
    r20 = print_check(lower, max_bridge = 20)
    assert_true(r20.bridge_area > r.bridge_area, msg = "the 6-ft head bridges at 20 mm")
    assert_true(r20.unsupported_area < r.unsupported_area)

def the_stories_are_pegged():
    # Pegs stand on each piece's top, `height` over the cut, and the pieces
    # have no ceilings: the upper story's only flat span is its floor's.
    for i, (top, label) in enumerate([(9.5 - -0.5, "main.0"), (19.0 - 9.5, "main.1")]):
        lo, hi = pieces[i][1].bbox()
        assert_close(hi[2], top * S + PEGS["height"], tol = 1e-3, msg = "pegs over the top of " + label)
    # The upper story's only overhang is its 6-ft window head (19 mm, past
    # the default max_bridge): no ceiling.
    assert_close(print_check(pieces[1][1]).unsupported_area, 6 * T * S * S, tol = 1e-3, msg = "no ceiling over the upper story")

test("every story is one solid, fits an H2D bed, stands on it, on the kit layer", every_piece_is_one_solid_on_the_bed)
test("the print takes walls and floors and leaves the glass and the furniture", the_selection_leaves_glass_and_furniture_out)
test("the 3-ft window head is a bridge at the default max_bridge; the 6-ft one is at 20 mm", the_three_foot_head_is_a_bridge_and_the_six_foot_one_is_not)
test("each story carries pegs into the plate above, and no story has a ceiling", the_stories_are_pegged)
