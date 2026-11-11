# The viewer benchmark's document, and the shape of it is the point.
#
# What is being measured is what the *page* does with an evaluation, not what
# the engine costs to produce one, so this script is deliberately trivial for
# the engine and deliberately expensive for the viewer:
#
#   * ~888 triangles per body, so 40 bodies is 35 520 -- within 1 % of
#     `examples/gridfinity_baselid.star`, a real document of that size;
#   * 40 separate bodies, because that is the case the base lid is *not*:
#     the lid is one body, and a model with many is where reusing the
#     unchanged ones can pay at all;
#   * three parameters that each change a different amount of the model, so
#     the benchmark's three cases are three slider moves rather than three
#     different documents.
#
# It must stay cheap to evaluate. A cylinder is a primitive, there is no
# boolean anywhere, and the whole thing is a few milliseconds natively -- so
# the benchmark's wall clock is the apply path and not the kernel.

count = param("count", 40, min=1, max=64, step=1, doc="How many bodies")
one = param("one", 0.0, min=0.0, max=8.0, step=0.5, unit="mm",
            doc="Extra height on the first body only: the edit that changes exactly one body")
every = param("every", 0.0, min=0.0, max=8.0, step=0.5, unit="mm",
              doc="Extra height on every body: the edit that changes all of them")

R = 3.0
PITCH = 9.0
COLS = 8

for i in range(count):
    h = 10.0 + every + (one if i == 0 else 0.0)
    cylinder(R, h, at=(i % COLS * PITCH, i // COLS * PITCH, 0.0), name="pin_%d" % i)
