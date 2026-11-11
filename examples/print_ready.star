# A small two-colour part that is checked before it is exported: a Gridfinity-
# sized tray with an inset label plate, exported as a 3MF that opens in the
# slicer already coloured.
#
#   cadpole eval    examples/print_ready.star            # the checks print as warnings
#   cadpole eval    examples/print_ready.star --param printer=prusa_mk4
#   cadpole export  examples/print_ready.star --run-exports --slicer bambu
#   cadpole render  examples/print_ready.star --analysis thickness -o /tmp/thick.png
#   cadpole render  examples/print_ready.star --analysis overhang  -o /tmp/over.png
#   cadpole check   examples/print_ready.star --orient
#
# Units are millimetres.

w = param("width", 42, unit="mm", min=20, max=120, doc="Tray width (x)")
d = param("depth", 42, unit="mm", min=20, max=120, doc="Tray depth (y)")
h = param("height", 18, unit="mm", min=8, max=60, doc="Tray height (z)")
wall = param("wall", 1.6, unit="mm", min=0.8, max=4, doc="Wall thickness")
floor = param("floor", 1.2, unit="mm", min=0.6, max=4, doc="Floor thickness")
label_h = param("label_height", 5, unit="mm", min=3, max=12, doc="Label plate height")
machine = param("printer", "bambu_a1",
                choices=["bambu_a1", "bambu_a1_mini", "bambu_p1s", "bambu_x1c", "prusa_mk4", "ender3"],
                doc="Which machine to check against")

# --- the tray ---------------------------------------------------------------
# Walls and floor come out of the difference, so `wall` really is the wall the
# thickness check will measure.
shell = box(w, d, h, name="shell")
pocket = box(w - 2 * wall, d - 2 * wall, h, at=(wall, wall, floor), name="pocket")
tray = subtract(shell, pocket, name="tray")
tray.material(color="#3E6B8A", roughness=0.7, label="body")

# --- the label plate --------------------------------------------------------
# A second body in a second filament: 1.2 mm thick (three perimeters of a
# 0.4 mm nozzle), half of it buried in the front wall so the two colours fuse
# instead of merely touching, and half standing proud.
plate = box(w - 12, 1.2, label_h, at=(6, -0.6, h - label_h - 2), name="label_plate")
plate.material(color="#E8C547", roughness=0.4, label="label")

# --- per-face paint ---------------------------------------------------------
# The tray's top rim in the label colour too, so the part reads as one design.
# `faces(normal=...)` picks the upward faces; paint records a feature, and both
# the viewport and the 3MF read it from there.
paint(tray, faces=tray.faces(normal=(0, 0, 1), within=1), color="#E8C547", slot=2)

# --- checks -----------------------------------------------------------------
# Never raises: each finding is a warning with a position, and the report is a
# value to assert on.
# `printer=` supplies the bed, the nozzle, the layer height and the material;
# `min_wall` and `max_overhang` spelled out here still win over it, which is
# how a profile stays a starting point rather than a mode.
report = print_check(tray, printer=machine, min_wall=0.8, max_overhang=45, orient=True)

assert_solid(tray, msg="the tray must be a solid")
assert_true(report.watertight, msg="the tray must be a closed solid")
assert_true(report.fits_bed, msg="the tray must fit the bed")
assert_true(report.min_wall >= 0.8, msg="walls thinner than two perimeters")
# A box tray has nothing but vertical walls and flat tops, so nothing needs
# support. If that ever changes this fails and says where.
assert_true(report.unsupported_area == 0.0, msg="the tray should need no support")

# The profile also carries the material, so the report can say what the part
# will weigh and how many layers tall it is.
assert_true(report.profile == machine)
assert_true(report.mass > 0)
assert_true(report.layers == int(h / printer(machine).layer_height))

label_report = print_check(plate, printer=machine, min_wall=0.8)
assert_true(label_report.watertight)
# Measured by ray casting through a float mesh, so compare with a margin.
assert_true(label_report.min_wall > 1.1, msg="the label plate is three perimeters thick")

# --- export -----------------------------------------------------------------
# Both bodies in one file: two filament slots, the tray's rim painted into the
# second. `--slicer bambu` adds Bambu/Orca's paint data on top of the 3MF core
# colours; the default `prusa` layer is always written and is valid 3MF.
export([tray, plate], "print_ready.3mf")
export([tray, plate], "print_ready.stl")
# And STEP, for whoever wants the exact surfaces rather than triangles. Both
# bodies are plain extrudes and a boolean the exact kernel handled, so they are
# still on the B-rep path, which is what STEP needs.
export([tray, plate], "print_ready.step")
