# Sample document for the real-engine end-to-end test (real-engine.spec.ts).
#
# Unlike the dev server's built-in script -- which is written for `MockEngine`,
# a text scraper rather than an interpreter -- this one uses the actual
# Starlark API (docs/starlark-api.md) so the wasm engine can evaluate it. It
# deliberately uses three different feature kinds, so the feature tree proves
# the model came from the engine and not from the mock:
#
#   plate   box
#   boss    extrude
#   ring    revolve

width = param("width", 60, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 40, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 12, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 14, min=1, max=100, step=0.5, unit="mm", doc="Boss height")

plate = box(width, depth, height, center=True, name="plate")

# The boss starts 1 mm inside the plate: coplanar faces are fragile in the
# current kernel, so overlap instead of touching.
boss = extrude(sketch(plane(origin=(0, 0, height - 1))).circle(boss_d / 2), boss_h, name="boss")

# A collar revolved about Z, from a rectangle sketched on XZ.
ring = revolve(sketch(XZ).rect(4, 3, center=(boss_d / 2 + 8, height + 2)), axis=(0, 0, 1), name="ring")
