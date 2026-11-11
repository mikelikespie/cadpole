# A part that is too tall for the machine, cut so it fits, exported for the
# slicer *and* for CAD.
#
# It exercises a named printer profile, `split()` and STEP export.
#
#   cadpole eval   examples/split_for_bed.star
#   cadpole eval   examples/split_for_bed.star --param printer=bambu_a1_mini
#   cadpole eval   examples/split_for_bed.star --param cut=False    # see it refuse
#   cadpole export examples/split_for_bed.star --run-exports --out-dir /tmp
#   cadpole export examples/split_for_bed.star --format step -o /tmp/stand.step
#   cadpole check  examples/split_for_bed.star
#
# Units are millimetres.

machine = param("printer", "bambu_a1",
                choices=["bambu_a1", "bambu_a1_mini", "bambu_p1s", "bambu_x1c", "prusa_mk4", "ender3"],
                doc="Which machine to check against")
height = param("height", 420, unit="mm", min=100, max=900, doc="Post height (z)")
side = param("side", 40, unit="mm", min=20, max=150, doc="Post cross-section")
plate = param("plate", 120, unit="mm", min=60, max=250, doc="Base plate width")
bolt = param("bolt", 5.5, unit="mm", min=3, max=12, doc="Bolt hole diameter in the base plate")
cut = param("cut", True, doc="Actually split the post; False shows the diagnostic instead")

# --- the machine ------------------------------------------------------------
# `printer(...)` reads the same table `print_check(printer="bambu_a1")` does,
# as a value you can look at. Build volume, nozzle and layer height are the
# vendor's published numbers and `.source` says which page they came from; the
# 45 degree overhang threshold is Cadpole's own default, not a vendor number.
p = printer(machine)
warn("checking against %s: %s x %s x %s mm bed, %s mm nozzle, %s mm layers, %s"
     % (p.display_name, p.bed[0], p.bed[1], p.bed[2], p.nozzle, p.layer_height, p.material))

# --- the base plate ---------------------------------------------------------
# Four bolt holes, drawn as loops in the sketch rather than cut with booleans:
# one `extrude`, and the whole plate stays on the B-rep path. This is the body
# that makes the STEP worth having -- each hole exports as a real
# CYLINDRICAL_SURFACE, so in FreeCAD it can be measured, tapped and filleted,
# where the same hole in an STL is a ring of flat facets.
inset = 10
holes = sketch(XY).rect(plate, plate, center=(0, 0))
for dx in [-1, 1]:
    for dy in [-1, 1]:
        holes = holes.circle(bolt / 2, center=(dx * (plate / 2 - inset), dy * (plate / 2 - inset)))
base = extrude(holes, 8, name="base")
base.material(color="#8A8F98", roughness=0.6)

base_report = print_check(base, printer=p)
assert_true(base_report.fits_bed, msg="the base plate should fit any of these machines")
assert_true(base_report.watertight)
# It warns about a "0.43 mm wall" at the rim of a bolt hole, and that warning
# is an artifact worth understanding rather than a bug to hide. Thickness is a
# ray down each triangle's inward normal plus four on an 18 degree cone
# (docs/design.md); on a triangle of the bore wall a fraction of a
# millimetre below the top face, one cone ray leaves through that face almost
# immediately. The real wall between a bore and the plate edge is 11 mm, and
# the total area affected is a few square millimetres of rim.
assert_true(base_report.thin_area < 50, msg="only the hole rims, not a real thin wall")

# --- the post ---------------------------------------------------------------
# A plain square post, deliberately taller than any of the beds.
post = box(side, side, height, at=(-side / 2, -side / 2, 8), name="post")
post.material(color="#5A7D9A", roughness=0.5)

before = print_check(post, printer=p)
assert_true(not before.fits_bed, msg="the point of this example is a part that does not fit")
warn("the post as modelled: %s, %s layers, %d g; %s"
     % (before.summary, before.layers, int(before.mass), before.split_advice))
assert_true(before.split_axis == "z", msg="a tall post is cut across z")
assert_true(before.split_pieces >= 2)

if not cut:
    # `--param cut=False` is the "what does it look like when it does not fit"
    # case: the bed-fit finding stays an error in the report and a warning in
    # the diagnostics, and the export still happens, because printing checks
    # are advice.
    export([base, post], "stand_whole.3mf")
else:
    # --- cut it -------------------------------------------------------------
    # `split()` takes the plane the check named. `before.split_at` is the list
    # of cut heights the check worked out; with two pieces there is one. The
    # pieces come back along the plane normal, so `lower` is the one below.
    #
    # `kernel="brep"` keeps both halves exact so they can still export STEP: a
    # body the mesh kernel made is triangles and cannot become exact surfaces
    # again (docs/design.md). The exact boolean cannot always cut a body with
    # curved faces; leave `kernel=` off and a cut it cannot do falls back to the
    # mesh kernel with a diagnostic, which prints but does not export STEP.
    z = before.split_at[0]
    lower, upper = split(post, plane(origin=(0, 0, z), normal=(0, 0, 1)), kernel="brep", name="piece")

    # Both pieces fit now, and each carries the face the cut exposed. The
    # naming grammar calls it `<feature>/cut` (docs/design.md), so a later
    # feature finds the glue face by name instead of by counting.
    for piece in [lower, upper]:
        r = print_check(piece, printer=p)
        assert_true(r.fits_bed, msg="each piece must fit the bed")
        assert_true(r.watertight, msg="a split must not open the solid")
        assert_true(len([f for f in piece.faces() if f.name.endswith("/cut")]) == 1,
                    msg="each piece has exactly one cut face")

    # Paint the glue faces: it shows in the viewport and survives into the 3MF.
    for piece in [lower, upper]:
        paint(piece, faces=[f for f in piece.faces() if f.name.endswith("/cut")], color="#C1553B", slot=2)

    # Stand the top half beside the bottom one instead of where the cut left it.
    upper = translate(upper, (side + plate / 2 + 10, 0, -z), name="upper_on_plate")

    # --- export -------------------------------------------------------------
    # 3MF for the slicer, with the glue faces already coloured.
    export([base, lower, upper], "stand_pieces.3mf")
    # STEP for a CAD tool: exact planes, and an exact cylinder for every bolt
    # hole. Every body has to be on the B-rep path for this to work.
    export([base, lower, upper], "stand_pieces.step")
