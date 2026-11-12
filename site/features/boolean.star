# Booleans: join, cut and keep-the-overlap, each from the same block and pin.
# The faint shapes are the inputs a result no longer shows.
def block(x, y, grow=0):
    return extrude(sketch(XY).rounded_rect(20 + 2 * grow, 20 + 2 * grow, 2 + grow), 10 + 2 * grow) \
        .translate((x, y, -grow))

def pin(x, y, grow=0):
    return cylinder(7 + grow, 14 + 2 * grow, at=(x + 5, y, -2 - grow))

def ghost(body):
    return body.material(color="#cfe9e1", opacity=0.1, roughness=0.2, shadow=False)

a1, b1 = block(-28, -9), pin(-28, -9)
a2, b2 = block(0, 18), pin(0, 18)
a3, b3 = block(28, -9), pin(28, -9)
# snippet
joined = union(a1, b1, name="joined")
cut = subtract(a2, b2, name="cut")
overlap = intersect(a3, b3, name="overlap")
# end snippet
joined.material(color="#6fbfa8", roughness=0.4)
cut.material(color="#6fbfa8", roughness=0.4)
overlap.material(color="#f0c987", roughness=0.4)

# What each result left behind: the pin the cut removed (a hair inside the
# slot it made), and both inputs round the overlap (a hair outside it).
tool = ghost(pin(0, 18, grow=-0.1))
ghost(block(28, -9, grow=0.1))
ghost(pin(28, -9, grow=0.1))

def exact_and_consistent():
    for b in (joined, cut, overlap):
        assert_solid(b)
        assert_eq(b.kind, "brep", msg="%s stays exact" % b.name)
    # Cut and overlap split the block between them; the join adds the pin.
    v_block = extrude(sketch(XY).rounded_rect(20, 20, 2), 10).volume()
    v_pin = math.pi * 7 * 7 * 14
    assert_close(cut.volume() + overlap.volume(), v_block, rel=0.01)
    assert_close(joined.volume(), v_block + v_pin - overlap.volume(), rel=0.01)
    # The pin runs past the block top and bottom and out of its end, so every
    # contact is a clean crossing.
    assert_bbox(joined, (-38, -19, -2), (-16, 1, 12))
    c = distance(cut, tool)
    assert_true(not c.overlap, msg="the removed pin's ghost sits in the slot, clear of the cut")

test("three exact results that account for the same block and pin", exact_and_consistent)
