# Sample document for engine.spec.ts (where evaluation runs).
#
# Two jobs. It must be evaluable by the *real* wasm engine, so the local side
# of the test is the real thing; and it must let the test tell the dev server
# how to behave on the remote side, without needing `SetScript` -- which the UI
# does not expose in session mode.
#
# The dev server reads `server_fail` and `server_slow_ms` out of the document's
# parameter overrides when it answers `Evaluate`, so the test drives them from
# the Parameters panel the way a user drags a slider. The engine ignores them:
# to the script they are just two more declared parameters.
#
# The body is called `plate`. The dev server's stand-in model is one body
# called `blk`, so which engine produced what is visible in the feature tree
# rather than only in a status line.

size = param("size", 30, min=10, max=60, step=1, unit="mm", doc="Plate width and depth (X and Y)")
thickness = param("thickness", 8, min=2, max=40, step=0.5, unit="mm", doc="Plate thickness (Z)")

server_fail = param("server_fail", False, doc="Dev server only: answer Evaluate with an error, so the browser has to fall back to its local engine")
server_slow_ms = param("server_slow_ms", 0, min=0, max=8000, step=100, unit="ms", doc="Dev server only: how long Evaluate should take, so there is something in flight to supersede or cancel")
server_colors = param("server_colors", False, doc="Dev server only: answer with a mesh that carries a colour per vertex, as an imported scan's does")

# How much work the *local* engine has to do, for the engine-swap measurements: with
# `holes` at 0 this script is a single box and evaluates in milliseconds, which
# is right for every test that is about where evaluation runs and useless for
# one that is about what re-evaluating costs after an engine swap. Drilling N
# holes is the cheapest way to buy real seconds, because `subtract` folds its
# operands pairwise and is quadratic in N -- the same cost shape as a document
# with many cuts, `examples/gridfinity_baselid.star` for one.
holes = param("holes", 0, min=0, max=32, step=1, doc="Cut this many holes, to make the local engine do measurable work")

if holes > 0:
    step = size / (holes + 1)
    tools = [
        cylinder(0.75, thickness * 2, at=(-size / 2 + step * (i + 1), 0, -thickness), name="hole_%d" % i)
        for i in range(holes)
    ]
    plate = subtract(box(size, size, thickness, center=True), tools, name="plate")
else:
    plate = box(size, size, thickness, center=True, name="plate")
