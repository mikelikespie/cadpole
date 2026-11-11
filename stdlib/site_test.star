# Tests for @cadpole/site.star: the geo frame against known arc-second
# lengths, a parcel's named sides, setbacks by area, the rear-yard line, a
# profile placed on the lot and read back off the ground, the envelope's
# cap and its volume on a slope (a mesh from the TIN, an exact body from the
# fit with no boolean), cut and fill against the closed form, a neighbour
# mass on its ground. Run by cadpole_script_test's every_stdlib_test_passes.

load(
    "@cadpole/site.star",
    "cut_fill",
    "envelope_above_grade",
    "footprint_mass",
    "ft",
    "grade_profile",
    "inch",
    "latlon_to_local",
    "local_frame",
    "local_to_latlon",
    "lot_point",
    "parcel",
    "polygon_area",
    "profile_points",
    "rear_yard",
    "setbacks",
    "to_ft",
    "to_mm",
)

frame = local_frame(38.0, -121.0, units = "ft")

# A 40 x 70 ft lot, corners given as lat/lon by way of the inverse, front
# along the x axis.
lot = parcel(
    frame,
    [local_to_latlon(frame, x, y) for x, y in [(0, 0), (40, 0), (40, 70), (0, 70)]],
    front = 0,
    name = "lot",
)
lot_body = extrude(lot["sketch"], ft(1), name = "lot_slab")

# A 30 % slope falling toward the rear (+y), flat across, sampled on a grid
# wider than the lot; its TIN reproduces the plane exactly.
shots = []
for i in range(6):
    for j in range(9):
        x = -10 + i * 14.0
        y = -10 + j * 12.5
        shots.append((x, y, 100 - 0.3 * y))
grade = terrain(to_mm(frame, shots), method = terrain.tin, name = "grade")
smooth = terrain(to_mm(frame, shots), method = terrain.fit, name = "smooth")

# The same slope as a surveyed profile down the lot plus a few shots
# beyond the side lines, the way site data often comes: profile_points puts
# the profile on the side lines and the centreline.
profile = [(v, 100 - 0.3 * v) for v in [-10, 0, 15, 30, 45, 60, 70, 80]]
beyond = [(-12, 5, 100 - 1.5), (52, 5, 100 - 1.5), (-12, 65, 100 - 19.5), (52, 65, 100 - 19.5)]
from_profile = terrain(profile_points(lot, to_mm(frame, profile)) + to_mm(frame, beyond), name = "from_profile")

buildable = setbacks(lot, front = ft(5), rear = ft(15), sides = ft(3), name = "buildable")
yard = rear_yard(lot, ft(42), name = "yard")
envelope = envelope_above_grade(grade, lot, ft(30), cap = ft(1000), base = -ft(20), name = "envelope")
capped = envelope_above_grade(grade, lot, ft(30), cap = ft(100), base = -ft(20), name = "capped")
exact = envelope_above_grade(smooth, lot, ft(30), base = -ft(20), name = "exact")
exact_capped = envelope_above_grade(smooth, lot, ft(30), cap = ft(100), base = -ft(20), name = "exact_capped")
pad = cut_fill(grade, lot, ft(90), base = -ft(20), name = "pad")
neighbour = footprint_mass(frame, [(50, 0), (80, 0), (80, 40), (50, 40)], ft(95), ft(28), latlon = False, name = "neighbour")

def geo():
    # One arc-second at 38 degrees: 30.83 m = 101.16 ft north, 24.4 m =
    # 80.05 ft east (the spheroid series; a spherical earth would say 101.4).
    x, y = latlon_to_local(frame, 38.0 + 1 / 3600.0, -121.0)
    assert_close(x, 0.0, tol = 1e-9)
    assert_close(y, 101.16, tol = 0.1)
    x, y = latlon_to_local(frame, 38.0, -121.0 + 1 / 3600.0)
    assert_close(x, 80.05, tol = 0.1)
    assert_close(y, 0.0, tol = 1e-9)
    # The inverse is the inverse.
    lat, lon = local_to_latlon(frame, 123.4, -56.7)
    assert_close(latlon_to_local(frame, lat, lon), (123.4, -56.7), tol = 1e-6)
    # A frame turned 90 degrees counter-clockwise has +y pointing west, so a
    # point due east is at negative y.
    turned = local_frame(38.0, -121.0, rotation_deg = 90, units = "ft")
    x, y = latlon_to_local(turned, 38.0, -121.0 + 1 / 3600.0)
    assert_close(x, 0.0, tol = 1e-9)
    assert_close(y, -80.05, tol = 0.1)
    # Metres and feet agree.
    metric = local_frame(38.0, -121.0, units = "m")
    assert_close(latlon_to_local(metric, 38.0 + 1 / 3600.0, -121.0)[1] / 0.3048, 101.16, tol = 0.1)
    assert_close(ft(1), 304.8)
    assert_close(inch(12), ft(1))
    assert_close(to_ft(ft(40)), 40.0)

test("one arc-second is 101 ft north and 80 ft east at 38 degrees", geo)

def parcel_names():
    assert_eq(lot["edge_names"], ["front", "side[0]", "rear", "side[1]"])
    assert_eq(lot["front_index"], 0)
    assert_eq(lot["rear_index"], 2)
    assert_close(lot["width"], ft(40), tol = 1e-6)
    assert_close(lot["depth"], ft(70), tol = 1e-6)
    assert_close(lot["area"], ft(40) * ft(70), rel = 1e-9)
    assert_close(polygon_area(lot["corners"]), lot["area"])
    assert_eq(lot["faces"]["front"], "side[0.0]")
    assert_eq(lot["faces"]["rear"], "side[0.2]")
    # The named sides are faces of the extruded lot.
    assert_true(lot_body.face("lot_slab/" + lot["faces"]["front"]) != None)
    assert_true(lot_body.face("lot_slab/" + lot["faces"]["rear"]) != None)
    # Corners the other way round come out counter-clockwise, and the
    # front stays the front: edge 3 of the clockwise list is edge 3 of the
    # reversed one, the names go round from it, and the rear is opposite.
    back = parcel(frame, [(0, 0), (0, 70), (40, 70), (40, 0)], front = 3, latlon = False, name = "back")
    assert_eq(back["front_index"], 3)
    assert_eq(back["edge_names"], ["side[0]", "rear", "side[1]", "front"])
    assert_eq(back["edge_names"][back["rear_index"]], "rear")
    assert_close(back["lines"]["front"], lot["lines"]["front"], tol = 1e-6)
    assert_close(back["lines"]["rear"], lot["lines"]["rear"], tol = 1e-6)
    assert_close(back["width"], ft(40), tol = 1e-6)
    assert_true(polygon_area(back["corners"]) > 0, msg = "counter-clockwise")

test("a parcel names its front, rear and sides", parcel_names)

def setback_area():
    # (40 - 3 - 3) x (70 - 5 - 15)
    assert_close(buildable["area"], ft(34) * ft(50), rel = 1e-9)
    assert_eq(len(buildable["corners"]), 4)
    assert_close(buildable["lines"]["front"][0][1], ft(5), tol = 1e-6)
    assert_close(buildable["lines"]["rear"][0][1], ft(55), tol = 1e-6)

test("setbacks clip the lot to the buildable area", setback_area)

def rear_yard_line():
    # 42 ft from the front: the line, the area in front of it, the names.
    assert_close(yard["depth"], ft(42))
    assert_close(yard["line"], ((0.0, ft(42)), (ft(40), ft(42))), tol = 1e-6)
    assert_close(yard["lines"]["rear"], yard["line"], tol = 1e-6)
    assert_close(yard["lines"]["front"], lot["lines"]["front"], tol = 1e-6)
    assert_close(yard["area"], ft(40) * ft(42), rel = 1e-9)
    assert_eq(len(yard["corners"]), 4)
    # The lot's own coordinates on the site's: across, then depth.
    assert_close(lot_point(lot, ft(10), ft(20)), (ft(10), ft(20)), tol = 1e-6)

test("the rear-yard line is parallel to the front at its depth", rear_yard_line)

def profiles():
    # The profile lands on both side lines and the centreline, in order.
    pts = profile_points(lot, to_mm(frame, profile))
    assert_eq(len(pts), 3 * len(profile))
    assert_close(pts[3], (0.0, 0.0, ft(100)), tol = 1e-6)
    assert_close(pts[4], (ft(20), 0.0, ft(100)), tol = 1e-6)
    assert_close(pts[5], (ft(40), 0.0, ft(100)), tol = 1e-6)
    # And the terrain built from it is the slope, on the lot and beyond.
    assert_close(from_profile.z_at(ft(20), ft(40)), ft(100 - 12), tol = 1e-3)
    assert_close(from_profile.z_at(ft(50), ft(30)), ft(100 - 9), tol = ft(0.5))
    assert_close(from_profile.average_grade(lot["corners"]), 0.3, tol = 1e-3)
    # Reading the ground back along the depth every 10 ft.
    line = grade_profile(grade, lot, step = ft(10))
    assert_eq(len(line), 8)
    assert_close(line[0], (0.0, ft(100)), tol = 1e-6)
    assert_close(line[4], (ft(40), ft(88)), tol = 1e-6)
    assert_close(line[7], (ft(70), ft(79)), tol = 1e-6)
    side = grade_profile(grade, lot, across = 0, step = ft(35), start = ft(35))
    assert_close(side, [(ft(35), ft(89.5)), (ft(70), ft(79))], tol = 1e-6)

test("a profile is placed on the lot and read back off the ground", profiles)

def envelope_volumes():
    # Grade at the front line is 100 ft (y = 0), falling 0.3 per ft; the
    # envelope from z = -20 ft to grade + 30 ft over the 40 x 70 lot:
    # 40 * integral(150 - 0.3 y) dy from 0 to 70 = 40 * (10500 - 735).
    assert_solid(envelope)
    assert_volume(envelope, ft(40) * ft(1) * ft(1) * (10500 - 735), rel = 0.002)
    # Capped at 100 ft: at the front the rule allows 130 ft, so the cap
    # holds until 100 = 130 - 0.3 y, y = 100 ft -- beyond the lot, so the
    # capped envelope is the box from -20 to 100 ft.
    assert_solid(capped)
    assert_volume(capped, ft(40) * ft(70) * ft(120), rel = 0.002)
    lo, hi = capped.bbox()
    assert_close(hi[2], ft(100), tol = 1.0)
    assert_close(lo[2], -ft(20), tol = 1.0)
    # The grade query the envelope was built from.
    assert_close(grade.z_at(ft(20), ft(40)), ft(100 - 12), tol = 1e-3)
    assert_close(grade.slope_at(ft(20), ft(40)), 0.3, tol = 1e-9)
    assert_close(grade.average_grade(lot["corners"]), 0.3, tol = 1e-6)
    # The lot is a rectangle on the site's axes, so the uncapped envelope is
    # the raised surface clipped to it: no boolean, the slab's own faces.
    assert_eq(sorted([f.name for f in envelope.faces()]), ["envelope/bottom", "envelope/side", "envelope/top"])

test("the envelope is grade plus 30 ft, capped", envelope_volumes)

def exact_envelope():
    # From the fitted surface the same envelope is one exact B-rep: the
    # patch cut to the lot by knot insertion, four planar sides, a bottom.
    assert_eq(exact.kind, "brep")
    assert_solid(exact)
    assert_eq(sorted([f.name for f in exact.faces()]), ["exact/bottom", "exact/side[0]", "exact/side[1]", "exact/side[2]", "exact/side[3]", "exact/top"])
    assert_volume(exact, ft(40) * ft(1) * ft(1) * (10500 - 735), rel = 0.005)
    lo, hi = exact.bbox()
    assert_close(lo, (0.0, 0.0, -ft(20)), tol = 1.0)
    assert_close(hi, (ft(40), ft(70), ft(130)), tol = ft(0.1))
    # A cap that bites runs on the mesh kernel and is the box.
    assert_eq(exact_capped.kind, "mesh")
    assert_solid(exact_capped)
    assert_volume(exact_capped, ft(40) * ft(70) * ft(120), rel = 0.005)
    # The fit reproduces the plane, and its clip is the same surface.
    assert_close(smooth.z_at(ft(20), ft(40)), ft(88), tol = 1.0)
    assert_close(smooth.clip(((0, 0), (ft(40), ft(70)))).z_at(ft(20), ft(40)), smooth.z_at(ft(20), ft(40)), tol = 1e-6)

test("a fitted terrain's envelope over a rectangular lot is exact", exact_envelope)

def earthwork():
    # A pad at 90 ft on the 30 % slope: cut where the ground is above it
    # (the front 33.3 ft), fill behind. Closed forms, 40 ft wide:
    # cut = 40 * int_0^33.33 (10 - 0.3 y) dy = 40 * 166.67 ft^3,
    # fill = 40 * int_33.33^70 (0.3 y - 10) dy = 40 * 201.67 ft^3.
    assert_true(pad["cut"] != None and pad["fill"] != None, msg = "both a cut and a fill")
    assert_solid(pad["cut"])
    assert_solid(pad["fill"])
    cu_ft = ft(1) * ft(1) * ft(1)
    assert_close(pad["cut_volume"], 40 * 166.6667 * cu_ft, rel = 0.005)
    assert_close(pad["fill_volume"], 40 * 201.6667 * cu_ft, rel = 0.005)
    lo, hi = pad["cut"].bbox()
    assert_close(lo[2], ft(90), tol = 1.0)
    assert_close(hi[1], ft(33.3333), tol = ft(0.05))
    lo, hi = pad["fill"].bbox()
    assert_close(hi[2], ft(90), tol = 1.0)
    assert_close(lo[1], ft(33.3333), tol = ft(0.05))
    # A pad above all the ground is all fill, one below it all cut.
    high = cut_fill(grade, lot, ft(110), base = -ft(20), name = "high")
    assert_true(high["cut"] == None and high["cut_volume"] == 0.0 and high["fill"] != None)
    assert_close(high["fill_volume"], 40 * (10 * 70 + 0.15 * 70 * 70) * cu_ft, rel = 0.005)
    low = cut_fill(grade, lot, ft(70), base = -ft(20), name = "low")
    assert_true(low["fill"] == None and low["cut"] != None)
    assert_close(low["cut_volume"], 40 * (30 * 70 - 0.15 * 70 * 70) * cu_ft, rel = 0.005)

test("cut and fill for a level pad match the closed forms", earthwork)

def neighbour_mass():
    assert_solid(neighbour)
    lo, hi = neighbour.bbox()
    assert_close(lo[2], ft(95), tol = 0.01)
    assert_close(hi[2], ft(95 + 28), tol = 0.01)
    assert_close(lo[0], ft(50), tol = 0.01)
    assert_volume(neighbour, ft(30) * ft(40) * ft(28), rel = 1e-6)

test("a footprint mass stands on its ground elevation", neighbour_mass)
