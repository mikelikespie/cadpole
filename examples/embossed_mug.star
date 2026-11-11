# Embossed mugs: bring your existing art, wrap it round a part.
#
# The tadpole mark is an ordinary SVG (assets/tadpole.svg, the logo with ids
# added so a script can pick its shapes) and the wave is a one-motif SVG
# (assets/wave.svg). Both come in through `import_svg` as exact sketches,
# then `wrap(..., method=wrap.unroll)` lays them on the *developed* surface
# of the mug's cylinder -- the sketch's millimetres become arc lengths, the
# way a label wraps a can -- and `emboss` turns them into relief:
#
#   * raised (`emboss.raised`): the art stands proud of the wall, its tops an
#     exact offset cylinder, glazed a second colour;
#   * cut (`emboss.cut`): the same loops sunk into the wall;
#   * painted (`emboss.paint`): no geometry, the face is split along the
#     loops and the pieces coloured -- a decal.
#
# Next to them stands the flat source: the same imported sketches embossed
# on a plain plaque, so one picture reads "flat file -> wrapped part".
#
# Neither a raise nor a cut is a boolean: the loops are imprinted into the
# face and the walls built edge for edge from them, so the result is an exact
# B-rep body. Two rules come with that, and the script keeps to both: art may
# not cross the cylinder's seam (a cylinder here is two half faces meeting on
# the x axis, where the handle sits), and it may not wrap most of the way
# round. The wave band therefore goes on as two half bands, one per half
# face, with the motifs spaced so no motif straddles a seam.
#
#   cadpole render embossed_mug.star -o mugs.png
#   cadpole render embossed_mug.star -p scene=single -p finish=cut -p handle=d -o one.png
#   cadpole check embossed_mug.star -p scene=single -p decals=false

diameter = param("diameter", 82.0, unit="mm", min=60, max=110, doc="Outside diameter of the mug")
height = param("height", 96.0, unit="mm", min=70, max=130, doc="Height of the mug")
wall = param("wall", 3.2, unit="mm", min=2.4, max=6, doc="Wall thickness")
floor_t = param("floor", 5.0, unit="mm", min=3, max=10, doc="Floor thickness")
handle = param("handle", "loop", choices=["loop", "d", "none"], doc="Handle style")
art_scale = param("art_scale", 1.0, min=0.5, max=1.4, doc="Size of the tadpole mark (1 = 50 mm wide)")
art_angle = param("art_angle", -25.0, unit="deg", min=-45, max=45, doc="Where the mark sits round the mug: 0 faces front, + turns toward the handle")
art_z = param("art_z", 58.0, unit="mm", min=30, max=100, doc="Height of the mark's centre")
relief = param("relief", 1.0, unit="mm", min=0.4, max=1.6, doc="How far a raised mark stands out or a cut one sinks in")
waves = param("waves", 10, min=6, max=16, step=2, doc="Wave motifs round the band (an even count: half on each half face)")
band_z = param("band_z", 13.0, unit="mm", min=8, max=30, doc="Height of the wave band's centre")
finish = param("finish", "raised", choices=["raised", "cut", "paint"], doc="Finish of the mug when scene=single")
decals = param("decals", True, doc="Show the art's colour in renders with thin overlay bodies (cadpole render draws each body in one colour; the paint itself is in the model for 3MF/GLB)")
scene = param("scene", "trio", choices=["trio", "single"], doc="trio: raised, cut and painted mugs beside the flat source art; single: one mug, for printing")

r = diameter / 2.0
assert_true(wall < r / 4, msg="the wall is too thick for that diameter")
assert_true(art_z < height - 8 and band_z > 4, msg="the art must stay clear of the rim and the foot")

# --- the art -----------------------------------------------------------------
# SVG px are CSS px (96 dpi): the 128-px logo tile is 33.9 mm, and the
# tadpole's silhouette (the `tadpole` group: body circle + tail) spans
# 105 x 71 px of it. Scale so it comes out LOGO_W wide.
PX = 25.4 / 96.0
LOGO_W = 50.0 * art_scale
k = LOGO_W / (105.0 * PX)       # import_svg scale
mm = PX * k                     # one SVG px in sketch millimetres
# The silhouette's bounding-box centre, in SVG px, and the eye and pupil
# centres; with flip_y the sketch's +y is the SVG's -y.
SIL_C = (72.5, 63.5)

def svg_at(x, y):
    """An SVG point (px) in the sketch, the silhouette centred on the origin."""
    return ((x - SIL_C[0]) * mm, (SIL_C[1] - y) * mm)

def tadpole(name, plane=XY, at=(0, 0)):
    """The mark as one sketch: the silhouette with the eye as a hole and the
    pupil as an island inside it -- three imports of one file, by id."""
    def place(p):
        q = svg_at(p[0], p[1])
        return (at[0] + q[0], at[1] + q[1])
    sil = import_svg(asset("tadpole.svg"), ids=["tadpole"], plane=plane, align="center", at=at, scale=k, name=name + "_silhouette")
    eye = import_svg(asset("tadpole.svg"), ids=["eye"], plane=plane, align="center", at=place((60, 49)), scale=k, name=name + "_eye")
    pupil = import_svg(asset("tadpole.svg"), ids=["pupil"], plane=plane, align="center", at=place((62.5, 49)), scale=k, name=name + "_pupil")
    return union2d(subtract2d(sil, eye, name=name + "_eyed"), pupil, name=name)

circ = math.tau * r
per_half = waves // 2
pitch = circ / waves            # arc length from one motif to the next
WAVE_W = 20.0                   # wave.svg is 20 x 11 mm
wave_k = min(1.0, 0.8 * pitch / WAVE_W)   # shrink the motif on a small mug so motifs never touch

def wave_band(name, count=None, plane=XY, at=(0, 0)):
    """`count` waves (a half band by default), centred on `at`, `pitch`
    apart along x."""
    n = count if count != None else per_half
    one = import_svg(asset("wave.svg"), plane=plane, align="center", at=(at[0] - (n - 1) * pitch / 2.0, at[1]), scale=wave_k, name=name + "_motif")
    return one.pattern2d_linear(n, (pitch, 0), name=name)

# The mark's angular half-width, and the band's, must stay inside a half face.
logo_half_deg = math.degrees(LOGO_W / 2.0 / r)
band_half_deg = math.degrees(((per_half - 1) * pitch + WAVE_W * wave_k) / 2.0 / r)
assert_true(abs(art_angle) + logo_half_deg < 85, msg="the mark would cross the seam under the handle: smaller, or nearer the front")
assert_true(band_half_deg < 90, msg="the wave band is wider than a half face")

# --- the mug -----------------------------------------------------------------
def on_wall(theta, z):
    """The point on the outside wall `theta` degrees from front (-y) toward +x."""
    return (r * math.sin(theta), -r * math.cos(theta), z)

def side_face(body, body_name, theta):
    """The outer half face of the wall at `theta`, found by a point on it
    clear of every piece of art (just under the lip)."""
    return body.faces(name=body_name + "_outer/side").nearest(on_wall(theta, height - 4.0))

def shell(name):
    outer = cylinder(r, height, name=name + "_outer")
    cavity = cylinder(r - wall, height, at=(0, 0, floor_t), name=name + "_cavity")
    s = subtract(outer, cavity, name=name + "_shell", kernel="brep")
    s = fillet(s, wall * 0.42, edges=s.edges(on_plane=XY.offset(height)), name=name + "_lip")
    s = fillet(s, 2.0, edges=s.edges(on_plane=XY), name=name + "_foot")
    return s

def make_handle(name):
    zt = height * 0.78
    zb = max(height * 0.30, band_z + 14.0)
    x0 = r - wall / 2.0
    x1 = r + 7.0
    if handle == "loop":
        rr = (zt - zb) / 2.0
        route = curve((x0, 0, zt)).line((x1, 0, zt)).arc((x1, 0, zb), via=(x1 + rr, 0, (zt + zb) / 2.0)).line((x0, 0, zb))
    else:
        c = 9.0
        q = c * (1.0 - math.sqrt(0.5))
        route = (curve((x0, 0, zt)).line((x1 + 6.0, 0, zt))
                 .arc((x1 + 6.0 + c, 0, zt - c), via=(x1 + 6.0 + c - q, 0, zt - q))
                 .line((x1 + 6.0 + c, 0, zb + c))
                 .arc((x1 + 6.0, 0, zb), via=(x1 + 6.0 + c - q, 0, zb + q))
                 .line((x0, 0, zb)))
    profile = sketch(route.start_plane, name=name + "_section").slot(13.0, 8.5)
    return sweep_along(profile, route, name=name)

# The art placements on one mug: the mark, and the band in two halves (one
# on the front half face, one on the back, each centred on its face so the
# seams fall between motifs). Each is (name, sketch builder, angle, height).
def placements(name):
    return [
        (name + "_mark", lambda n: tadpole(n), art_angle, art_z),
        (name + "_band0", lambda n: wave_band(n), 0.0, band_z),
        (name + "_band1", lambda n: wave_band(n), 180.0, band_z),
    ]

def decal(name, art, theta, z, outer_r, film_t):
    """A thin coloured body in the shape of `art` as it lies on the mug,
    its outside at radius `outer_r`: the same wrap and the same raise on a
    fresh cylinder of the mug's radius (so the loops sit at the mug's
    angles), all but the outer `film_t` of the relief taken away again, and
    the shell left scaled radially into place. (A raised mark's decal is
    only its top: the side walls would coincide with the relief's own, and
    growing the loops with `Sketch.offset` leaves a sliver wall face the
    tessellator refuses, so the walls stay the mug's colour.)

    Only for the picture: `emboss(color=)` and `emboss.paint` colour the
    faces in a 3MF or GLB export, but `cadpole render` draws a body in one
    colour, so the colour is shown by these."""
    core = cylinder(r, height, name=name + "_core")
    sk = art(name + "_sketch")
    face = core.faces(name=name + "_core/side").nearest(on_wall(theta, height - 4.0))
    w = wrap(sk, face, method=wrap.unroll, origin=on_wall(theta, z), up=(0, 0, 1), name=name + "_wrap")
    raised = emboss(core, face, w, relief, kind=emboss.raised, name=name + "_raised")
    film = subtract(raised, cylinder(r + relief - film_t, height + 2, at=(0, 0, -1), name=name + "_trim"), name=name + "_film", kernel="mesh")
    s = outer_r / (r + relief)
    return scale(film, (s, s, 1), name=name)

def mug(kind, name, glaze, art_color):
    body = shell(name)
    films = []
    for (pname, art, theta, z) in placements(name):
        face = side_face(body, name, theta)
        placed = wrap(art(pname), face, method=wrap.unroll, origin=on_wall(theta, z), up=(0, 0, 1), name=pname + "_wrap")
        if kind == "paint":
            body = emboss(body, face, placed, kind=emboss.paint, color=art_color, name=pname + "_art")
        else:
            body = emboss(body, face, placed, relief, kind=kind, color=art_color, name=pname + "_art")
        if decals:
            if kind == "raised":
                d = decal(pname + "_decal", art, theta, z, r + relief + 0.2, 0.1)
            elif kind == "cut":
                d = decal(pname + "_decal", art, theta, z, r - relief + 0.2, 0.6)
            else:
                d = decal(pname + "_decal", art, theta, z, r + 0.2, 0.6)
            d.material(preset="petg_glossy", color=art_color, roughness=0.18)
            films.append(d)
    if handle != "none":
        body = union(body, make_handle(name + "_handle"), name=name)
    body.material(preset="petg_glossy", color=glaze, roughness=0.18)
    return [body] + films

# Glazes: cream with teal relief, teal with cream cut into it, mustard with
# deep-teal paint.
STYLES = {
    "raised": ("#efe7d6", "#14967f"),
    "cut": ("#14967f", "#f3ecdc"),
    "paint": ("#e7b33e", "#0e6e5e"),
}

def plaque(name):
    """The flat source: the same two SVGs extruded on a flat tile standing on
    edge -- what the mugs are wrapped from. The art is its own body, the way
    a two-colour print carries it."""
    w = LOGO_W + 22.0
    t = 4.0
    mark_h = 71.0 * mm             # the silhouette is 71 px tall
    wave_h = 11.0 * wave_k
    tile_h = mark_h + wave_h + 24.0
    tile = extrude(sketch(XY, name=name + "_outline").rounded_rect(w, tile_h, 5.0, center=(0, 0)), t, name=name + "_tile")
    tile.material(preset="pla_matte", color="#f7f4ee")
    mark = extrude(tadpole(name + "_mark", plane=XY.offset(t), at=(0, tile_h / 2.0 - 7.0 - mark_h / 2.0)), relief, name=name + "_mark_relief")
    row = extrude(wave_band(name + "_waves", count=3, plane=XY.offset(t), at=(0, -tile_h / 2.0 + 7.0 + wave_h / 2.0)), relief, name=name + "_waves_relief")
    out = []
    for b in [tile] + mark + row:
        b2 = rotate(b, 90, axis=(1, 0, 0))
        out.append(translate(b2, (0, 0, tile_h / 2.0)))
    for b in out[1:]:
        b.material(preset="pla_matte", color="#14967f")
    return out

MUGS = {}
if scene == "single":
    MUGS[finish] = mug(finish, "mug", STYLES[finish][0], STYLES[finish][1])[0]
else:
    gap = (36.0 if handle != "none" else 14.0)
    step = diameter + gap
    order = ["raised", "cut", "paint"]
    for i, kind in enumerate(order):
        bs = mug(kind, "mug_" + kind, STYLES[kind][0], STYLES[kind][1])
        moved = [b.translate(((i - 1) * step, 0, 0)) for b in bs]
        MUGS[kind] = moved[0]
    for b in plaque("source"):
        b.translate((-1.5 * step - LOGO_W * 0.5 - 2.0, -diameter * 0.3, 0))

# --- tests -------------------------------------------------------------------
# The art's area, from the same sketches the mugs were wrapped with.
ART_AREA = tadpole("t_mark").area() + 2 * wave_band("t_band").area()

def relief_volume():
    """A raised mug holds about `relief` x the art's area more than the
    painted one and a cut mug that much less (a little more and less: the
    relief is cut from a cylinder, so its tops are r +- relief). Volumes are
    measured on the tessellation of a ~200 cm3 mug, so a difference of a
    few cm3 carries some of that chord error: 6 % is the tolerance."""
    raised = MUGS["raised"].volume()
    cut = MUGS["cut"].volume()
    painted = MUGS["paint"].volume()
    nominal = ART_AREA * relief
    assert_close(raised - painted, nominal * (1 + relief / (2 * r)), rel=0.06, msg="raised relief volume")
    assert_close(painted - cut, nominal * (1 - relief / (2 * r)), rel=0.06, msg="cut pocket volume")
    return raised - cut

def mug_size():
    for kind, b in MUGS.items():
        lo, hi = b.bbox()
        assert_close(hi[2] - lo[2], height, tol=0.05, msg=kind + ": the mug is `height` tall")
        assert_close(lo[2], 0.0, tol=0.05, msg=kind + ": it stands on z = 0")
        assert_true(hi[1] - lo[1] <= diameter + 2 * relief + 0.05, msg=kind + ": nothing but the art stands proud of the wall")
        assert_solid(b, msg=kind + ": a closed solid")

test("relief volume matches the art", relief_volume, sweep={"diameter": [64, 82, 100], "art_scale": [0.7, 1.0, 1.3]})
test("more art, more relief", relief_volume, sweep={"art_scale": [0.6, 0.8, 1.0, 1.2, 1.4]}, monotone="increasing")
test("mug size and solidity", mug_size, sweep={"diameter": [64, 82, 100], "handle": ["loop", "d", "none"]})

animation("scroll_story", [
    keyframe(0.0, scene="single", finish="paint", relief=0.4, ease=ease.linear, camera=view(az=-58, el=14, fov=24)),
    keyframe(1.2, finish="paint", relief=0.4, camera=view(az=-40, el=14, fov=24)),
    keyframe(1.25, finish="raised", relief=0.4),
    keyframe(2.6, finish="raised", relief=1.6, camera=view(az=-14, el=18, fov=24)),
    keyframe(2.65, finish="cut", relief=1.6),
    keyframe(3.6, finish="cut", relief=1.0, camera=view(az=8, el=22, fov=24)),
], fps=20)
