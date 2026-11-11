# cadpole: bench case -- boolean fan-out.
#
# One body, many small tools, in one `subtract` call. This is the shape of
# `examples/gridfinity_baselid.star`'s magnet pockets -- 32 sockets cut from a
# lid in a single call -- reduced until nothing else in it costs anything: the
# body is a twelve-triangle box, so every millisecond measured here belongs to
# the boolean path and to nothing else.
#
# Run it at several `n` and read the *shape*, not the number. A fold that does
# one whole-body boolean per tool is quadratic in `n`, and doubling `n` then
# costs four times as much; one boolean against the tools' union is linear, and
# doubling `n` costs twice. The baseline records n = 4, 8, 16, 32 and 64 for
# exactly that reason -- a single size cannot tell the two apart.
#
# The tools are laid out on a grid with a clear gap between them, which is what
# lets them be batched: bodies that cannot touch each other cannot cut each
# other, so their union is their concatenation.

n = param("n", 32, min=1, max=256, doc="How many tools the one subtract call takes")
kernel = param("kernel", "mesh", choices=["auto", "brep", "mesh"],
               doc="Which kernel does the fan-out; \"brep\" measures the exact path, which has no batch")
tool = param("tool", "cylinder", choices=["cylinder", "box"],
             doc="What the tools are. A magnet socket is a cylinder; the exact kernel refuses a fan of those past the fourth (its tangent case, contact handling in docs/design.md), so the B-rep fold is measured with prisms")

PITCH = 12.0
COLS = 16
PLATE = 200.0

plate = box(PLATE, PLATE, 10.0, at=(-PLATE / 2, -PLATE / 2, -5.0), name="plate")

tools = []
for i in range(n):
    cx = -90.0 + PITCH * (i % COLS)
    cy = -90.0 + PITCH * (i // COLS)
    # Taller than the plate at both ends: neither end lands on a face, so the
    # cut is transversal and no contact case is in the way of the measurement.
    if tool == "box":
        tools.append(box(6.0, 6.0, 20.0, at=(cx - 3.0, cy - 3.0, -10.0), name="hole_%d" % i))
    else:
        tools.append(cylinder(3.0, 20.0, at=(cx, cy, -10.0), name="hole_%d" % i))

perforated = subtract(plate, tools, name="perforated", kernel=kernel)

# The holes really were cut: a plate of n holes has lost n cylinders of it.
lo, hi = perforated.bbox()
assert_true(math.close(hi[2] - lo[2], 10.0, tol=1e-6), msg="the tools cut through, not off")
