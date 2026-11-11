# A composed scene: four parts laid out two by two on the same grid, for
# renders and for exercising the renderer with mixed geometry (revolved
# spline surfaces, booleans, patterns) in one frame.

spacing = param("spacing", 115, unit="mm", min=60, max=300, doc="Grid spacing between the four parts")

# A tapered planter with a drainage hole (revolved profile).
planter = revolve(
    sketch(XZ, name="planter_profile").polygon([
        (4, 0), (42, 0), (52, 70), (49.5, 70), (39.5, 3.5), (4, 3.5),
    ]),
    axis=(0, 0, 1),
    name="planter",
)
planter.material(color=(0.80, 0.52, 0.40), roughness=0.85)

# A vase from a smooth spline profile.
vase_profile = (
    path((3, 0))
    .h(28)
    .spline([(44, 46), (23, 76), (30, 92)], tension=0.5)
    .h(27.5)
    .spline([(20.5, 76), (41.5, 46), (26, 3.5)], tension=0.5)
    .h(3)
)
vase = revolve(sketch(XZ, name="vase_profile").add(vase_profile), axis=(0, 0, 1), name="vase")
vase = translate(vase, (spacing, 10, 0))
vase.material(color=(0.72, 0.78, 0.82), roughness=0.3)

# A bracket: booleans over a plate and a boss.
plate = box(70, 34, 7, center=True, at=(14, spacing, 0), name="plate")
boss = cylinder(9, 12, at=(14, spacing, 6), name="boss")
body = union(plate, boss, name="bracket_body")
holes = [
    cylinder(3, 9, at=(14 - 26, spacing, -1), name="hole_l"),
    cylinder(3, 9, at=(14 + 26, spacing, -1), name="hole_r"),
]
bracket = subtract(body, holes, name="bracket")
bracket.material(color=(0.62, 0.66, 0.72), roughness=0.45, metallic=0.6)

# A tray of pegs, from one peg copied across a grid.
tray = box(90, 60, 5, center=True, at=(spacing + 6, spacing + 6, 0), name="tray")
tray.material(color=(0.78, 0.74, 0.58), roughness=0.7)
peg = cylinder(4, 16, at=(spacing - 24, spacing - 12, 5), name="peg")
peg.material(color=(0.55, 0.62, 0.55), roughness=0.6)
for i, p in enumerate(pattern_linear(peg, 3, (0, 18, 0), name="peg_row")):
    pattern_linear(p, 4, (20, 0, 0), name="peg_col_%d" % i)
