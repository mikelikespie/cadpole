# A 608 deep-groove ball bearing -- the skateboard bearing: bore 8, outside
# diameter 22, width 7, seven balls of 3.97 mm on a 15.1 mm pitch circle.
# Two races, each the revolve of a rectangular groove profile, and the
# balls as exact spheres on a circular pattern; nothing touches anything, so
# every body is its own solid and the file is nine of them.
#
# This is the part `examples/vendor_bearing.star` imports back: the
# STEP this script exports is `examples/assets/bearing_608.step`, so the
# round trip is Cadpole reading what Cadpole wrote, and the test below is the
# same test the importer's side runs on the file.
#
#   cadpole test examples/bearing_608.star
#   cadpole export examples/bearing_608.star --run-exports --out-dir examples/assets
#   cadpole render examples/bearing_608.star --view iso -o /tmp/bearing_608.png

bore = param("bore", 8.0, unit="mm", min=3, max=20, doc="Bore diameter")
od = param("od", 22.0, unit="mm", min=10, max=60, doc="Outside diameter")
width = param("width", 7.0, unit="mm", min=3, max=20, doc="Width")
ball_d = param("ball_d", 3.97, unit="mm", min=1, max=12, doc="Ball diameter")
balls = param("balls", 7, min=3, max=16, doc="Number of balls")
pitch_d = param("pitch_d", 15.1, unit="mm", min=6, max=50, doc="Pitch circle diameter, ball centre to ball centre")
play = param("play", 0.05, unit="mm", min=0.01, max=0.3, doc="Radial gap between a ball and the bottom of each groove")

r_ball = ball_d / 2.0
r_pitch = pitch_d / 2.0
h = width / 2.0
# The lands sit a little way in from the ball's equator so the groove
# shoulders hold the ball; the groove is as wide as the ball's chord at the
# land radius plus the play.
shoulder = 0.7 * r_ball
land_in = r_pitch - shoulder
land_out = r_pitch + shoulder
half_chord = math.sqrt(r_ball * r_ball - shoulder * shoulder)
g = half_chord + play
groove_in = r_pitch - r_ball - play
groove_out = r_pitch + r_ball + play

# Sketch x is radial, sketch y is axial (the XZ plane revolved about z).
inner_profile = sketch(XZ, name="inner_profile").polygon([
    (bore / 2.0, -h), (land_in, -h), (land_in, -g), (groove_in, -g),
    (groove_in, g), (land_in, g), (land_in, h), (bore / 2.0, h),
])
outer_profile = sketch(XZ, name="outer_profile").polygon([
    (land_out, -h), (od / 2.0, -h), (od / 2.0, h), (land_out, h),
    (land_out, g), (groove_out, g), (groove_out, -g), (land_out, -g),
])
inner = revolve(inner_profile, axis=(0, 0, 1), name="inner_race")
outer = revolve(outer_profile, axis=(0, 0, 1), name="outer_race")
ball = sphere(r_ball, center=(r_pitch, 0, 0), name="ball")
# The original is the first of the list, so this is every ball.
ball_set = pattern_circular(ball, balls, axis=(0, 0, 1), name="balls")

inner.material(preset=preset.steel)
outer.material(preset=preset.steel)
for b in ball_set:
    b.material(preset=preset.steel)

export([inner, outer] + ball_set, "bearing_608.step")


def races_are_rings_and_balls_are_spheres():
    assert_solid(inner)
    assert_solid(outer)
    assert_solid(ball)
    # A revolved rectangle-with-a-groove is a ring: its volume is the
    # rectangle's minus the groove's, each swept round its own centroid.
    # The mesh a volume is measured on is inscribed in the cylinders, so at
    # the display tolerance a race reads a few tenths of a percent small.
    ring = math.pi * width * ((land_in * land_in) - (bore / 2.0) * (bore / 2.0))
    cut = math.pi * (2.0 * g) * ((land_in * land_in) - (groove_in * groove_in))
    assert_close(inner.volume(), ring - cut, rel=5e-3)
    ring = math.pi * width * ((od / 2.0) * (od / 2.0) - (land_out * land_out))
    cut = math.pi * (2.0 * g) * ((groove_out * groove_out) - (land_out * land_out))
    assert_close(outer.volume(), ring - cut, rel=5e-3)
    # A 2 mm ball at the display tolerance is a coarse polyhedron: 5 % small.
    assert_close(ball.volume(), 4.0 / 3.0 * math.pi * r_ball * r_ball * r_ball, rel=0.06)
    assert_eq(ball.faces(kind=kind.sphere).count(), 2)
    # Four cylindrical segments per profile, three faces per full turn.
    assert_eq(inner.faces(kind=kind.cylinder).count(), 12)
    assert_eq(outer.faces(kind=kind.cylinder).count(), 12)
    assert_eq(len(ball_set), balls)
    return inner.volume() + outer.volume()


test("the races are grooved rings and the balls are spheres", races_are_rings_and_balls_are_spheres, sweep={"balls": [5, 9]})


def nothing_touches():
    # The balls sit in their grooves with the play on every side, so no two
    # bodies touch: each body is a solid on its own and the STEP is nine of them.
    assert_true(not distance(ball, inner).overlap, msg="a ball must not touch the inner race")
    assert_true(not distance(ball, outer).overlap, msg="a ball must not touch the outer race")
    assert_true(distance(ball, inner).distance >= play * 0.5, msg="the play is real")
    assert_true(distance(ball, ball_set[1]).distance > 0.5, msg="the balls are apart")


test("no ball touches a race or another ball", nothing_touches)
