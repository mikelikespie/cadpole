# A surveyor's site plan, imported, and a lot frame built from it.
#
# `examples/assets/site_survey.dxf` is a survey the way a surveyor's CAD
# writes one -- and a file Cadpole's own DXF writer wrote, in feet:
# the property lines as one closed polyline on `PL`, contour lines one per
# layer named for its elevation (`CONTOUR-150` ... `CONTOUR-156`), twenty
# spot shots as `TEXT` labels on `SPOT` in the four spellings a survey uses
# (`152.37`, `TC 152.37`, `152.37 BK`, `EG 152.37`), a bench mark on
# `CONTROL`, three tree canopies as circles on `TREE`, a dashed building
# setback on `BLDG-SETBACK`, and notes whose numbers are not elevations.
#
# `import_dxf` reads it once into a `Drawing2D`, keyed by the file's content
# hash through the memo, and the script questions it layer by layer: the
# lot outline is a sketch (one loop), the contours come back with their
# elevations, the spot shots as (x, y, z), every coordinate in millimetres
# because `$INSUNITS` says feet. What is built from it: a frame around the
# lot -- the property lines offset inward, the ring between extruded -- a
# survey stake on every spot shot, as tall as the shot is above the bench
# mark, and the tree canopies as discs. Terrain from the shots and the
# contours is `terrain(...)` / `terrain_from_contours(...)`.
#
#   cadpole test examples/site_survey.star
#   cadpole inspect examples/site_survey.star
#   cadpole render examples/site_survey.star --view iso -o docs/demo/site_survey.png

frame_width = param("frame_width", 4.0, unit="ft", min=1, max=10, doc="Width of the lot frame, inside the property lines")
frame_height = param("frame_height", 1.0, unit="ft", min=0.25, max=4, doc="Height of the lot frame")
stake_base = param("stake_base", 1.0, unit="ft", min=0.25, max=4, doc="A stake's height at the bench mark's elevation")

site = import_dxf("site_survey.dxf", name="site")

# `$INSUNITS` said feet, so the drawing's scale is millimetres per foot; the
# script's own feet go through the same number.
ft = site.scale

# The property lines: the one closed loop on PL, in the survey's coordinates.
lot = site.sketch(layer="PL", closed=True)

# The lot frame: the property lines offset inward by the frame's width, and
# the ring between the two extruded. One sketch with a hole, so the frame is
# one exact prism and no 3D boolean is needed.
inset = lot.offset(-frame_width * ft, join=join.miter, name="inset")
ring = subtract2d(lot, inset, name="ring")
frame = extrude(ring, frame_height * ft, name="frame")
frame.material(preset=preset.pla_matte, color="#b9b1a3")

# The shots. The bench mark on CONTROL is the datum every stake stands
# above; the twenty spot shots on SPOT are where the surveyor stood.
bench_mark = site.spot_elevations(layer="CONTROL")[0]
datum = bench_mark[2]
shots = site.spot_elevations(layer="SPOT")
stakes = []
for i, (x, y, z) in enumerate(shots):
    stake = cylinder(0.3 * ft, z - datum + stake_base * ft, at=(x, y, 0), name="stake_%d" % i)
    stake.material(preset=preset.pla_matte, color="#e0762e")
    stakes.append(stake)

# The contours, with their elevations from the layer names.
contours = site.contours(layer="CONTOUR*")
low = min([z for z, pts in contours])
high = max([z for z, pts in contours])

# Tree canopies: the circles on TREE, as discs a foot thick.
canopies = extrude(site.sketch(layer="TREE", closed=True), 1.0 * ft, name="canopy")
for c in canopies:
    c.material(preset=preset.pla_matte, color="#4f8a45")


def the_survey_reads_as_a_lot_with_its_shots():
    assert_eq(site.units, "ft")
    assert_eq(site.unit_source, "$INSUNITS")
    assert_close(site.scale, 304.8)
    # The layer table, in the file's order: the writer's empty `0` first.
    assert_eq([l[0] for l in site.layers()][0:3], ["0", "PL", "PL-ANNO"])
    assert_eq(site.layers()[1], ("PL", 1, (255, 0, 0)))
    # The lot: one loop of five lines, named for its layer under the feature.
    assert_eq(lot.loops, 1)
    assert_eq(lot.paths, 0)
    assert_eq(lot.names, ["site/PL/loop[0]"])
    bb = site.bbox()
    assert_close((bb[1][0] - bb[0][0]) / ft, 132.0, tol=1e-6)
    # Seven contours a foot apart, every one a polyline of thirteen points.
    assert_eq(len(contours), 7)
    assert_close(low / ft, 150.0)
    assert_close(high / ft, 156.0)
    assert_eq([len(pts) for z, pts in contours], [13] * 7)
    # Twenty spot shots and the bench mark read as elevations; the bearings
    # on PL-ANNO and the notes ("LOT 12", "SCALE 1\" = 20'") do not.
    assert_eq(len(shots), 20)
    assert_eq(len(site.spot_elevations()), 21)
    assert_eq(len(site.spot_elevations(layer=["NOTES", "PL-ANNO"])), 0)
    assert_eq(len(site.spot_elevations(layer="SPOT", pattern="TC #")), 5)
    assert_close(datum / ft, 147.52)
    assert_true(all([z > datum for x, y, z in shots]), msg="every shot is above the bench mark")
    assert_eq(len(site.texts()), 26)
    # The setback is an open chain: a construction path, never a face.
    setback = site.sketch(layer="BLDG-SETBACK")
    assert_eq(setback.loops, 0)
    assert_eq(setback.paths, 1)
    # Every entity on every layer as a curve, named by kind and index.
    curves = site.curves(layer="PL")
    assert_eq(len(curves), 1)
    assert_eq(curves[0].id, "site/PL/lwpolyline[0]")
    assert_true(curves[0].closed)
    # The perimeter the plat's five courses add up to (the north line rises
    # 5' over its 100').
    assert_close(curves[0].length / ft, 110.0 + 46.098 + 36.056 + 100.125 + 70.0, tol=0.01)
    return len(shots)


test("the survey reads as a lot, seven contours, twenty shots and a bench mark", the_survey_reads_as_a_lot_with_its_shots)


def the_frame_is_a_ring_inside_the_property_lines():
    assert_solid(frame)
    fb = frame.bbox()
    # The frame's outer edge is the property line: the plat runs from the
    # south-west corner at (0, 0) to 120' east and 75' north.
    assert_close(fb[0][0] / ft, 0.0, tol=1e-6)
    assert_close(fb[0][1] / ft, 0.0, tol=1e-6)
    assert_close(fb[1][0] / ft, 120.0, tol=1e-6)
    assert_close(fb[1][1] / ft, 75.0, tol=1e-6)
    assert_close((fb[1][2] - fb[0][2]) / ft, frame_height, tol=1e-6)
    # A ring: less than a slab over the lot's extent, and more than nothing.
    assert_true(frame.volume() < 120.0 * 75.0 * frame_height * ft * ft * ft, msg="a ring is less than the slab")
    assert_true(frame.volume() > 0.0)
    return frame.volume()


test(
    "the frame is a solid ring on the property lines, wider frame more material",
    the_frame_is_a_ring_inside_the_property_lines,
    sweep={"frame_width": [2.0, 4.0, 6.0]},
    monotone=monotone.increasing,
)


def every_stake_stands_on_its_shot():
    assert_eq(len(stakes), 20)
    for i, stake in enumerate(stakes):
        assert_solid(stake)
        x, y, z = shots[i]
        # A bbox is measured on the tessellation, in f32: a hundredth of a
        # millimetre thirty metres from the origin.
        sb = stake.bbox()
        assert_close((sb[0][0] + sb[1][0]) / 2.0, x, tol=0.01)
        assert_close((sb[0][1] + sb[1][1]) / 2.0, y, tol=0.01)
        assert_close(sb[1][2], z - datum + stake_base * ft, tol=0.01)
    assert_eq(len(canopies), 3)
    for c in canopies:
        assert_solid(c)


test("every stake stands on its shot, as tall as the shot is above the bench mark", every_stake_stands_on_its_shot)
