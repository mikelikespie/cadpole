# Sample document for the viewport-lighting test (shading.spec.ts).
#
# One body per `sample`, each a surface the viewport has got wrong before: a
# dark metal (nothing but reflection lights it), a white matte part (the
# first to wash out), a painted part in several colours, and a dark matte
# one. The shape is the same for all four -- a flanged drum with a boss on
# top -- so there are faces towards the key light, away from it and curving
# between, and no boolean (the suite runs an unoptimized engine).
#
# A fifth, `film`, is for depth fighting instead: a round white sleeve whose
# bore clears the corners of a dark 24-sided post by 0.06 mm, the way artwork
# wrapped on a mug lies on a wall meshed differently from it. Two blocks a
# long way off put the camera across a room. Beside each corner the two
# surfaces are a hair apart and lie at different slopes, so a depth offset
# that grows with a face's slope draws the post through the sleeve there.

sample = param("sample", "dark_metal", choices=["dark_metal", "white_matte", "painted", "dark_matte", "film"])

if sample == "film":
    drum = extrude(sketch(XY).polygon([(100 * math.cos(math.radians(15 * i + 4)), 100 * math.sin(math.radians(15 * i + 4))) for i in range(24)]), 100, name="drum")
    drum.material(color="#2a2b2e", roughness=0.85)
    film = revolve(sketch(XZ).polyline([(100.06, 20), (100.26, 20), (100.26, 80), (100.06, 80)], closed=True), axis=(0, 0, 1), name="film")
    film.material(color="#f2f2f0", roughness=0.8)
    for i, x in enumerate([-420, 420]):
        box(10, 10, 10, center=True, name="post_%d" % i).translate((x, 0, 0)).material(color="#2a2b2e", roughness=0.85)
else:
    drum = revolve(sketch(XZ).polyline([(0, 0), (40, 0), (40, 6), (26, 6), (26, 44), (40, 44), (40, 50), (0, 50)], closed=True), axis=(0, 0, 1), name="drum")
    boss = box(30, 30, 16, center=True, name="boss").translate((0, 0, 48))

    if sample == "dark_metal":
        drum.material(color="#6c6470", metallic=1.0, roughness=0.5)
        boss.material(color="#8f7f6c", metallic=1.0, roughness=0.45)
    elif sample == "white_matte":
        drum.material(color="#f2f2f0", roughness=0.8)
        boss.material(color="#f2f2f0", roughness=0.8)
    elif sample == "painted":
        drum.material(color="#c8402f", roughness=0.5)
        boss.material(color="#2f6fc8", roughness=0.5)
    else:
        drum.material(color="#2a2b2e", roughness=0.85)
        boss.material(color="#2a2b2e", roughness=0.85)
