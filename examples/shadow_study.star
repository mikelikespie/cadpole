# A hillside lot study: where a proposed house throws its shadow on the
# neighbours, and what it takes from their view.
#
# An invented 50 x 100 ft lot at 38 N, out over open ocean at 140 W so
# the place is no real one, the
# street along its south edge, the ground stepping down 20 ft to a back
# terrace where an old studio with a roof lantern stands. The proposal is a
# 30 x 40 x 30 ft mass on the front terrace with a passage -- an open slot --
# between it and the east property line; the neighbour to the west has a
# light court against the shared line and a second-floor window looking
# east over the lot. The street and sidewalk run along the front, a
# retaining wall holds the front terrace, and the lot to the east is open.
#
# What this example demonstrates:
#
#   * **`sun(lat, lon, date, time, tz=)`**: the sun's azimuth and elevation
#     for a place and a clock time (NOAA's algorithm, to half a degree), and
#     `solstices(year)` / `equinoxes(year)` for the dates a study is drawn at;
#   * **`render_defaults(sun=, north=, camera=)`**: the document's own sun,
#     driven by the `date` and `hour` parameters, so `cadpole render` and a
#     `cadpole sweep --param hour=9,12,15` light the model with it; the camera is
#     the neighbour's window, so a plain `cadpole render` is the view from it;
#   * **`shadow_study(bodies, on=, sun=|dates=, times=)`**: the footprint the
#     house throws on the neighbour's light court -- exact for the
#     tessellation on a planar face -- and on the back terrace (a body, so
#     sampled), one footprint per sun, with `.area()` a test can assert on;
#   * **`sightline(from, to, obstacles=)`** and **`sightlines(from, target,
#     obstacles=)`**: the sidewalk's line of sight down the passage to the
#     studio's lantern, blocked at 8 ft and clear at 12, and the fraction of
#     the studio's front the sidewalk still sees, which the `passage` sweep
#     test holds to be monotone (`from` is a reserved word in Starlark, so
#     the eye is positional or `eye=`, the target positional or `to=`);
#   * **`view_cone(from, az=, fov=, el=)`**: how much of the eastern skyline
#     band the neighbour's window loses to the proposal;
#   * **an animation over `hour`**: `cadpole animate examples/shadow_study.star
#     --name day` sweeps the sun across the winter solstice.
#
#   cadpole test examples/shadow_study.star
#   cadpole render examples/shadow_study.star -o /tmp/window.png          # from the neighbour's window
#   cadpole render examples/shadow_study.star --view az=200,el=40 --sun 38,-140,2026-12-21,09:00,-9 -o /tmp/winter_morning.png
#   cadpole render examples/shadow_study.star --shadow-study --sun 38,-140,2026-06-21,12:00,-9 \
#       --view az=200,el=45 --size 480x360 --no-grid -o docs/demo/shadow_study.png
#   cadpole render examples/shadow_study.star --shadow-study --sun 38,-140,2026-01-01,00:00,-9 \
#       --at 2026-12-21,12:00 --at 2026-03-20,09:00 --at 2026-03-20,15:00 --view az=200,el=45 -o /tmp/three.png
#   cadpole render examples/shadow_study.star --view az=200,el=45 --section z=3000 --param hour=15 -o /tmp/plan_cut.png
#   cadpole animate examples/shadow_study.star --name day --view az=200,el=45 --no-grid --fps 10 -o /tmp/day.gif
#   cadpole sweep examples/shadow_study.star --param hour=9,12,15 --view az=200,el=45 -o /tmp/hours.png

ft = 304.8

LAT = 38.0
LON = -140.0
TZ = -9  # standard time; a summer study at -8 is the same sun an hour later on the clock

passage = param("passage", 8.0, unit="ft", min=4, max=20, doc="Width of the open slot between the house and the east line")
date = param("date", "2026-12-21", doc="The day the document's sun is computed for (YYYY-MM-DD)")
hour = param("hour", 9.0, unit="h", min=5, max=21, doc="The local clock time the document's sun is computed for")

# ---------------------------------------------------------------- the lot

# Two terraces: the front one at street level, the back one 20 ft down.
front_terrace = box(50 * ft, 50 * ft, 1 * ft, at=(0, 0, -1 * ft), name="front_terrace")
back_terrace = box(50 * ft, 50 * ft, 1 * ft, at=(0, 50 * ft, -21 * ft), name="back_terrace")
front_terrace.material(color="#7f9270", roughness=0.9)
back_terrace.material(color="#748763", roughness=0.9)
# The retaining wall that holds the front terrace up, the street and
# sidewalk in front, and the open lot to the east: context, drawn plainly.
retaining = box(50 * ft, 1 * ft, 21 * ft, at=(0, 49.5 * ft, -21 * ft), name="retaining_wall")
retaining.material(color="#8d8f8c", roughness=0.9)
sidewalk_slab = box(110 * ft, 6 * ft, 1 * ft, at=(-40 * ft, -6 * ft, -1 * ft), name="sidewalk")
street = box(110 * ft, 24 * ft, 1 * ft, at=(-40 * ft, -30 * ft, -1.5 * ft), name="street")
sidewalk_slab.material(color="#a9a8a2", roughness=0.9)
street.material(color="#5a5f63", roughness=0.95)
east_front = box(20 * ft, 50 * ft, 1 * ft, at=(50 * ft, 0, -1 * ft), name="east_lot_front")
east_back = box(20 * ft, 50 * ft, 1 * ft, at=(50 * ft, 50 * ft, -21 * ft), name="east_lot_back")
for b in [east_front, east_back]:
    b.material(color="#6c7d5d", roughness=0.95)

# The proposal: its east wall stands `passage` from the east property line.
house = box(30 * ft, 40 * ft, 30 * ft, at=(50 * ft - passage * ft - 30 * ft, 10 * ft, 0), name="house")
house.material(color="#d8cfbf", roughness=0.7)

# The old studio on the back terrace, with a lantern on its roof: the top
# of the lantern is 8 ft below a sidewalk eye.
studio = box(20 * ft, 20 * ft, 14 * ft, at=(28 * ft, 60 * ft, -20 * ft), name="studio")
lantern = box(4 * ft, 4 * ft, 3.5 * ft, at=(36 * ft, 68 * ft, -6 * ft), name="lantern")
studio.material(color="#c9b9a0", roughness=0.8)
lantern.material(color="#a8b8c8", roughness=0.5)
lantern_top = (38 * ft, 70 * ft, -2.5 * ft)

# The neighbour to the west: a house set back from the shared line by a
# 5 ft light court, and a second-floor window in its east wall.
# Its lot is a slab a little lower than the court's paving, so the two
# never share a face.
neighbour_lot = box(40 * ft, 100 * ft, 1 * ft, at=(-40 * ft, 0, -1.5 * ft), name="neighbour_lot")
neighbour_lot.material(color="#6c7d5d", roughness=0.95)
# Its east wall has the window, a 4 x 5 ft opening with glass set back in it;
# a gable roof runs along its length.
neighbour = subtract(
    box(25 * ft, 40 * ft, 25.5 * ft, at=(-30 * ft, 10 * ft, -0.5 * ft), name="neighbour_mass"),
    box(1 * ft, 4 * ft, 5 * ft, at=(-5.6 * ft, 28 * ft, 12.5 * ft), name="neighbour_window_opening"),
    name="neighbour",
)
neighbour_glass = box(0.1 * ft, 4 * ft, 5 * ft, at=(-5.5 * ft, 28 * ft, 12.5 * ft), name="neighbour_window")
neighbour_glass.material(color="#9fc4d6", roughness=0.1, opacity=0.6)
neighbour_roof = extrude(sketch(plane(origin=(0, 10 * ft, 0), normal=(0, 1, 0), x_dir=(1, 0, 0)), name="neighbour_roof_profile").polygon([
    (-31 * ft, -25 * ft), (-4 * ft, -25 * ft), (-17.5 * ft, -32 * ft),
]), 40 * ft, name="neighbour_roof")
neighbour_roof.material(color="#6d7178", roughness=0.7)
court = box(5 * ft, 20 * ft, 0.5 * ft, at=(-5 * ft, 20 * ft, -0.5 * ft), name="light_court")
neighbour.material(color="#cfc3b0", roughness=0.7)
court.material(color="#c8c4bc", roughness=0.9)
court_top = court.faces(normal=(0, 0, 1)).one()
window = (-5 * ft + 0.2 * ft, 30 * ft, 15 * ft)

# The sidewalk eye at the mouth of the passage, 5.5 ft up.
sidewalk = (50 * ft - passage * ft / 2, 0, 5.5 * ft)

# ---------------------------------------------------------------- the sun

# The document's sun, and the view from the neighbour's window.
render_defaults(
    sun=sun(LAT, LON, date, hour, tz=TZ),
    north=0,
    camera=view(eye=window, target=(45 * ft, 60 * ft, 0), fov=70),
)

winter, summer = solstices(2026)[1], solstices(2026)[0]
equinox = equinoxes(2026)[0]

# The nine-tile study on the neighbour's light court: three dates, three
# times. `cadpole render --shadow-study` draws the same nine as a contact sheet.
study = shadow_study(
    house,
    on=court_top,
    dates=[summer, equinox, winter],
    times=["09:00", "12:00", "15:00"],
    lat=LAT,
    lon=LON,
    tz=TZ,
)

# The footprint for the document's own sun, drawn on the court in an
# accent colour: the exact area a test asserts on, made visible. Guide
# geometry: the camera sees it and the light does not.
here = shadow_study(house, on=court_top, sun=sun(LAT, LON, date, hour, tz=TZ))[0]
court_shadow = None
if here.area() > 0:
    court_shadow = extrude(here.sketch(name="court_shadow_outline"), 20, name="court_shadow")
    court_shadow.material(color="#2fbf9f", roughness=0.6, shadow=False)
    layer("court_shadow", doc="The court's shadow footprint for the document's date and hour").add(court_shadow)

# The sidewalk's line of sight to the lantern, drawn: teal while it is
# clear, red from where the house cuts it. A small ball marks the eye.
line_of_sight = sightline(sidewalk, lantern_top, obstacles=[house, studio])
guides = layer("sightline", doc="The sidewalk eye and its line of sight to the studio's lantern")
R_LINE = 2.5 * 25.4
def rod(a, b, color, name):
    r = hull(sphere(R_LINE, at=a), sphere(R_LINE, at=b), name=name)
    r.material(color=color, roughness=0.5, shadow=False)
    return guides.add(r)

eye_ball = guides.add(sphere(5 * 25.4, at=sidewalk, name="sidewalk_eye"))
eye_ball.material(color="#f0c987", roughness=0.5, shadow=False)
if line_of_sight.blocked:
    rod(sidewalk, line_of_sight.hit, "#2fbf9f", "sight_clear")
    rod(line_of_sight.hit, lantern_top, "#e0604f", "sight_blocked")
else:
    rod(sidewalk, lantern_top, "#2fbf9f", "sight_clear")

# ---------------------------------------------------------------- tests

def court_is_clear_at_winter_noon():
    # The case a shadow objection turns on: at noon on the shortest
    # day the sun is due south, the house's shadow runs north onto its own
    # lot, and the light court to the west gets none of it.
    noon = shadow_study(house, on=court_top, sun=sun(LAT, LON, winter, "12:00", tz=TZ))
    assert_eq(noon[0].area(), 0, msg="the house adds no shadow on the light court at 12:00 on the winter solstice")
    assert_true(noon[0].lit, msg="the court faces the noon sun")
    assert_true(noon[0].exact, msg="a planar face gets the exact footprint")
    # The winter morning is the case the neighbour will raise: the low sun
    # in the south-east throws the house's shadow north-west, across the
    # court. The study's first row is summer, its last winter.
    morning = shadow_study(house, on=court_top, sun=sun(LAT, LON, winter, "09:00", tz=TZ))
    assert_true(morning[0].area() > 0, msg="the winter morning shadow does reach the court")
    assert_eq(len(study), 9)
    assert_eq(study.labels[8], winter + " 15:00")
    assert_true(study.max_area() >= morning[0].area() - 1e-6)

test("light court at winter noon", court_is_clear_at_winter_noon)

def back_terrace_takes_the_noon_shadow():
    # Where the noon shadow does go: 54 ft north of the house's north wall,
    # onto the back terrace. A body receiver is terrain, so this one is
    # sampled rather than exact.
    s = shadow_study(house, on=back_terrace, sun=sun(LAT, LON, winter, "12:00", tz=TZ))
    assert_true(s[0].area() > 20 * ft * 20 * ft, msg="area %s mm2" % s[0].area())
    assert_true(not s[0].exact)

test("back terrace at winter noon", back_terrace_takes_the_noon_shadow)

def sidewalk_sees_the_lantern_through_a_wide_mews():
    line = sightline(sidewalk, lantern_top, obstacles=[house, studio])
    if passage < 10:
        assert_true(line.blocked, msg="at %s ft the house's east wall crosses the line of sight" % passage)
        assert_eq(line.blocked_by, "house")
    else:
        assert_true(line.clear, msg="at %s ft the sidewalk sees the lantern: %s" % (passage, line))

test("lantern from the sidewalk", sidewalk_sees_the_lantern_through_a_wide_mews, sweep={"passage": [8, 12]})

def studio_front_seen_from_the_sidewalk():
    # The fraction of the studio's front the sidewalk still sees grows
    # with the passage: the sweep holds it monotone.
    front = studio.faces(normal=(0, -1, 0)).one()
    fan = sightlines(sidewalk, front, obstacles=[house], samples=400)
    return fan.fraction

test("studio front from the sidewalk", studio_front_seen_from_the_sidewalk, sweep={"passage": [8, 12, 16]}, monotone=monotone.increasing)

def the_window_loses_some_eastern_sky():
    before = view_cone(window, az=90, fov=120, el=(0, 25), obstacles=[studio, lantern], rays=24)
    after = view_cone(window, az=90, fov=120, el=(0, 25), obstacles=[house, studio, lantern], rays=24)
    # The house stands 17 ft from the window and 15 ft above it, so it
    # fills the band over the azimuths it spans; what is left is the sky
    # past its corners.
    assert_true(before.visible > 0.9, msg="without the house the band is open: %s" % before.visible)
    assert_true(after.visible < 0.5, msg="the house takes most of the skyline band: %s -> %s" % (before.visible, after.visible))
    assert_true(after.visible > 0.05, msg="but not all of it: %s" % after.visible)
    assert_eq(after.blocked_by[0][0], "house")

test("skyline from the neighbour's window", the_window_loses_some_eastern_sky)

# ---------------------------------------------------------------- a day

# The sun across the winter solstice, dawn to dusk, for `cadpole animate`.
animation("day", [
    keyframe(0, hour=7.5),
    keyframe(6, hour=17),
], fps=10, ease=ease.linear)
