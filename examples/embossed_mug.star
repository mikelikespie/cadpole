# Embossed mugs: bring your existing art, wrap it round a part.
#
# The tadpole mark is an ordinary SVG (assets/tadpole.svg, the logo with ids
# added so a script can pick its shapes: the silhouette, the dark tone drawn
# over it, the eye and the pupil) and the wave is a one-motif SVG
# (assets/wave.svg). Both come in through `import_svg` as exact sketches,
# then `wrap(..., method=wrap.unroll)` lays them on the *developed* surface
# of the mug's cylinder -- the sketch's millimetres become arc lengths, the
# way a label wraps a can -- and `emboss` turns them into relief:
#
#   * raised (`emboss.raised`): the art stands proud of the wall, its tops an
#     exact offset cylinder, glazed in the mark's two tones;
#   * cut (`emboss.cut`): the same loops sunk into the wall;
#   * painted (`emboss.paint`): no geometry, the face is split along the
#     loops and the pieces coloured -- a decal.
#
# The mark keeps the logo's colours as separate pieces of art, each embossed
# in its own colour, so a multi-colour print carries them all: the dark body
# (the eye open in it, so the glaze is its white), the light rim of the
# tail's paddle and the ink pupil. An imprint splits one face, so two pieces
# of art may not share an edge; the rim stops a hairline (`TONE_GAP`) short
# of the dark tone, the way a two-colour relief is drawn for printing anyway.
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
art_scale = param("art_scale", 1.0, min=0.5, max=1.4, doc="Size of the tadpole mark (1 = 44 mm tall)")
art_angle = param("art_angle", -25.0, unit="deg", min=-45, max=45, doc="Where the mark sits round the mug: 0 faces front, + turns toward the handle")
art_z = param("art_z", 56.0, unit="mm", min=30, max=100, doc="Height of the mark's centre")
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
# SVG px are CSS px (96 dpi): the 128-px logo tile is 33.9 mm, and the mark's
# silhouette (the `mark` path) spans 99.7 x 119.9 px of it, its box centred on
# SIL_C. Scale so it comes out LOGO_H tall.
PX = 25.4 / 96.0
SIL_W = 99.66
SIL_H = 119.93
SIL_C = (65.97, 64.0)
LOGO_H = 44.0 * art_scale
k = LOGO_H / (SIL_H * PX)       # import_svg scale
mm = PX * k                     # one SVG px in sketch millimetres
LOGO_W = SIL_W * mm
TONE_GAP = 0.3                  # mm between the two tones: art that shares an edge cannot be imprinted

def mark_part(id, name, plane, at):
    """One shape of the mark, in the file's own coordinates moved so the
    silhouette's box is centred on `at`. (With flip_y the file's y runs up
    from the bottom of its 128-px tile.)"""
    return import_svg(asset("tadpole.svg"), ids=[id], plane=plane, align="none",
                      at=(at[0] - SIL_C[0] * mm, at[1] - (128.0 - SIL_C[1]) * mm), scale=k, name=name)

def tadpole(name, plane=XY, at=(0, 0)):
    """The dark tone: the body with the eye open in it, so the glaze is the
    white of the eye."""
    body = mark_part("body", name + "_body", plane, at)
    eye = mark_part("eye", name + "_eye", plane, at)
    return subtract2d(body, eye, name=name)

def pupil(name, plane=XY, at=(0, 0)):
    """The pupil: an island in the eye, clear of the body all round."""
    return mark_part("pupil", name, plane, at)

def paddle_rim(name, plane=XY, at=(0, 0)):
    """The light tone: the paddle's rim, the silhouette less the dark tone,
    kept TONE_GAP clear of it."""
    sil = mark_part("mark", name + "_silhouette", plane, at)
    body = mark_part("body", name + "_body", plane, at)
    return subtract2d(sil, body.offset(TONE_GAP), name=name)

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

FOOT_RING = 6.0                 # width of the ring the mug stands on
FOOT_RECESS = 1.0               # depth of the shallow recess inside it

def shell(name):
    """The thrown form: a lip rounded over, the floor blended into the wall
    inside the way a wheel leaves it, and a foot ring under a shallow
    recess, so the mug stands on its rim and not on its whole base."""
    outer = cylinder(r, height, name=name + "_outer")
    cavity = cylinder(r - wall, height, at=(0, 0, floor_t), name=name + "_cavity")
    s = subtract(outer, cavity, name=name + "_shell", kernel="brep")
    s = fillet(s, wall * 0.42, edges=s.edges(on_plane=XY.offset(height)), name=name + "_lip")
    s = fillet(s, min(6.0, floor_t + 1.0, r * 0.15), edges=s.edges(on_plane=XY.offset(floor_t)), name=name + "_floor")
    s = fillet(s, 2.0, edges=s.edges(on_plane=XY), name=name + "_foot")
    recess = cylinder(r - FOOT_RING, FOOT_RECESS + 1.0, at=(0, 0, -1.0), name=name + "_recess_cut")
    return subtract(s, recess, name=name + "_recess", kernel="brep")

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
    profile = sketch(route.start_plane, name=name + "_section").slot(HANDLE_W, HANDLE_T)
    return [sweep_along(profile, route, name=name),
            handle_root(name + "_top", route.start_plane, 0.0),
            handle_root(name + "_bottom", route.start_plane, zb - zt)]

HANDLE_W = 13.0                 # the handle's section, a slot
HANDLE_T = 8.5

def handle_root(name, top_plane, dz):
    """Where the handle is pulled from the wall: a smooth loft that starts
    2.6 mm wider than the handle all round just under the wall and narrows
    onto the handle's own section 6 mm out, the slip blend a potter
    leaves. It starts inside the wall, so its edge meets the curved wall
    on a slant and leaves no ledge, and it ends a little inside the handle."""
    secs = []
    for (d, grow) in [(0.0, 2.6), (max(0.2, wall / 2.0 - 1.4), 2.6), (wall / 2.0 + 1.4, 1.2), (wall / 2.0 + 3.6, 0.45), (wall / 2.0 + 5.8, 0.12), (wall / 2.0 + 7.6, -0.3)]:
        secs.append(sketch(top_plane.offset(d), name=name + "_s%d" % len(secs)).slot(HANDLE_W + 2 * grow, HANDLE_T + 2 * grow))
    return translate(loft(secs, ruled=False, name=name + "_loft"), (0, 0, dz), name=name)

# The art placements on one mug: the mark's two tones, and the band in two
# halves (one on the front half face, one on the back, each centred on its
# face so the seams fall between motifs). Each is (name, sketch builder,
# angle, height, tone): tone 0 is the art colour, 1 the paddle's light rim,
# 2 the pupil.
def placements(name):
    return [
        (name + "_mark", lambda n: tadpole(n), art_angle, art_z, 0),
        (name + "_rim", lambda n: paddle_rim(n), art_angle, art_z, 1),
        (name + "_pupil", lambda n: pupil(n), art_angle, art_z, 2),
        (name + "_band0", lambda n: wave_band(n), 0.0, band_z, 0),
        (name + "_band1", lambda n: wave_band(n), 180.0, band_z, 0),
    ]

DECAL_FILM = 0.1                # thickness of the coloured film over the art
DECAL_LIFT = 0.2                # where the film's outside sits above the face it colours

def decal(name, art, theta, z, outer_r, inset=0.0, round_tips=0.0):
    """A thin coloured body in the shape of `art` as it lies on the mug,
    its outside at radius `outer_r` and its inside `DECAL_FILM` below it:
    the same wrap on a fresh cylinder of the mug's radius (so the loops sit
    at the mug's angles), raised by a fixed `h`, everything inside
    `r + h - DECAL_FILM` taken away again, and the film scaled radially into
    place. The raise is a constant and not the mug's `relief`: the trim
    cylinder has to clear the whole core, or a full-height tube of it
    survives into the film. (A raised mark's decal is only its top: the
    side walls would coincide with the relief's own, so they stay the mug's
    colour.)

    Only for the picture: `emboss(color=)` and `emboss.paint` colour the
    faces in a 3MF or GLB export, but `cadpole render` draws a body in one
    colour, so the colour is shown by these. `inset` shrinks the art first:
    a film on a pocket's floor would otherwise share the pocket's walls."""
    h = DECAL_FILM + 0.4
    core = cylinder(r, height, name=name + "_core")
    sk = art(name + "_sketch")
    if inset > 0 and round_tips > 0:
        # Shrunk past the inset and grown back: the mark's sharp tips come
        # out rounded, so a film's point stays off its pocket's walls.
        sk = sk.offset(-inset - round_tips, name=name + "_shrunk").offset(round_tips, name=name + "_inset")
    elif inset > 0:
        sk = sk.offset(-inset, name=name + "_inset")
    face = core.faces(name=name + "_core/side").nearest(on_wall(theta, height - 4.0))
    w = wrap(sk, face, method=wrap.unroll, origin=on_wall(theta, z), up=(0, 0, 1), name=name + "_wrap")
    raised = emboss(core, face, w, h, kind=emboss.raised, name=name + "_raised")
    film = subtract(raised, cylinder(r + h - DECAL_FILM, height + 2, at=(0, 0, -1), name=name + "_trim"), name=name + "_film", kernel="mesh")
    s = outer_r / (r + h)
    return scale(film, (s, s, 1), name=name)

def mug(kind, name, glaze, art_colors):
    body = shell(name)
    films = []
    for (pname, art, theta, z, tone) in placements(name):
        art_color = art_colors[tone]
        if tone == 2:
            # the eye is an island of the mark's raise or cut, a face of its own
            face = body.faces(name=name + "_mark_art/island*").nearest(on_wall(theta, z))
        else:
            face = side_face(body, name, theta)
        placed = wrap(art(pname), face, method=wrap.unroll, origin=on_wall(theta, z), up=(0, 0, 1), name=pname + "_wrap")
        if kind == "paint":
            body = emboss(body, face, placed, kind=emboss.paint, color=art_color, name=pname + "_art")
        else:
            body = emboss(body, face, placed, relief, kind=kind, color=art_color, name=pname + "_art")
        if decals:
            # The face the colour lies on: the relief's tops, the pocket's
            # floor, or the wall itself.
            under = r + relief if kind == "raised" else (r - relief if kind == "cut" else r)
            d = decal(pname + "_decal", art, theta, z, under + DECAL_LIFT, inset=0.15 if kind == "cut" else 0.0, round_tips=0.4 * min(1.0, art_scale) if tone < 2 and pname.endswith(("_mark", "_rim")) else 0.0)
            d.material(preset="petg_glossy", color=art_color, roughness=0.12)
            films.append(d)
    if handle != "none":
        (pulled, top_root, bottom_root) = make_handle(name + "_handle")
        body = union(body, pulled, top_root, bottom_root, name=name)
    # A satin glaze on the body and a glossy one on the art, so the art
    # separates by its sheen as well as its colour.
    body.material(preset="petg_glossy", color=glaze, roughness=0.34)
    return [body] + films

# Glazes: cream with teal relief, teal with cream cut into it, mustard with
# deep-teal paint; each with the lighter tone of the paddle's rim and an ink
# pupil.
INK = "#0b2e2a"
STYLES = {
    "raised": ("#efe7d6", ("#14967f", "#7fd8b8", INK)),
    "cut": ("#14967f", ("#f3ecdc", "#9fe6cc", INK)),
    "paint": ("#e7b33e", ("#0e6e5e", "#3cb592", INK)),
}

PLAQUE_W = max(LOGO_W + 22.0, 2 * pitch + WAVE_W * wave_k + 1.0)   # the mark, or a row of three waves

def plaque(name):
    """The flat source: the same two SVGs extruded on a flat tile standing on
    edge -- what the mugs are wrapped from. The art is its own body, the way
    a two-colour print carries it."""
    w = PLAQUE_W
    t = 4.0
    mark_h = LOGO_H
    wave_h = 11.0 * wave_k
    tile_h = mark_h + wave_h + 24.0
    tile = extrude(sketch(XY, name=name + "_outline").rounded_rect(w, tile_h, 5.0, center=(0, 0)), t, name=name + "_tile")
    tile.material(preset="pla_matte", color="#f7f4ee")
    mark_at = (0, tile_h / 2.0 - 7.0 - mark_h / 2.0)
    mark = extrude(tadpole(name + "_mark", plane=XY.offset(t), at=mark_at), relief, name=name + "_mark_relief")
    rim = extrude(paddle_rim(name + "_rim", plane=XY.offset(t), at=mark_at), relief, name=name + "_rim_relief")
    eye = extrude(pupil(name + "_pupil", plane=XY.offset(t), at=mark_at), relief, name=name + "_pupil_relief")
    row = extrude(wave_band(name + "_waves", count=3, plane=XY.offset(t), at=(0, -tile_h / 2.0 + 7.0 + wave_h / 2.0)), relief, name=name + "_waves_relief")
    mark = mark if type(mark) == "list" else [mark]
    rim = rim if type(rim) == "list" else [rim]
    colored = [(b, "#14967f") for b in mark] + [(b, "#7fd8b8") for b in rim] + [(eye, INK)] + [(b, "#14967f") for b in row]
    out = []
    for b, c in [(tile, None)] + colored:
        b2 = translate(rotate(b, 90, axis=(1, 0, 0)), (0, 0, tile_h / 2.0))
        if c:
            b2.material(preset="pla_matte", color=c)
        out.append(b2)
    return out

MUGS = {}
DECALS = {}                     # finish -> its colour films, for the tests
if scene == "single":
    bs = mug(finish, "mug", STYLES[finish][0], STYLES[finish][1])
    MUGS[finish] = bs[0]
    DECALS[finish] = bs[1:]
else:
    gap = (36.0 if handle != "none" else 14.0)
    step = diameter + gap
    order = ["raised", "cut", "paint"]
    for i, kind in enumerate(order):
        bs = mug(kind, "mug_" + kind, STYLES[kind][0], STYLES[kind][1])
        moved = [b.translate(((i - 1) * step, 0, 0)) for b in bs]
        MUGS[kind] = moved[0]
        DECALS[kind] = moved[1:]
    for b in plaque("source"):
        b.translate((-1.5 * step - PLAQUE_W * 0.5 + 9.0, -diameter * 0.3, 0))

# --- tests -------------------------------------------------------------------
# The art's area, from the same sketches the mugs were wrapped with.
ART_AREA = tadpole("t_mark").area() + paddle_rim("t_rim").area() + pupil("t_pupil").area() + 2 * wave_band("t_band").area()

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

def decals_sit_on_the_art():
    """Each colour film is only the art: no taller than the mark, clear of
    the mug's wall, and no further out than the face it colours. A film that
    let the trim cylinder miss the core would run the mug's full height."""
    mark_h = LOGO_H
    for kind, films in DECALS.items():
        assert_eq(len(films), len(placements("m")) if decals else 0, msg=kind + ": a film for each tone of the mark and each half band")
        for d in films:
            lo, hi = d.bbox()
            assert_true(hi[2] - lo[2] < mark_h + 1.0, msg="%s: %s is %s mm tall, more than the art" % (kind, d, hi[2] - lo[2]))
            c = distance(d, MUGS[kind])
            assert_true(not c.overlap, msg="%s: %s cuts into the mug at %s" % (kind, d, c.at))
            assert_true(c.distance < 0.5, msg="%s: %s floats %s mm off its face" % (kind, d, c.distance))

def thrown_details():
    """The foot ring and the handle's blended roots are really there."""
    for kind, b in MUGS.items():
        lo, hi = b.bbox()
        under = b.faces(normal=(0, 0, -1), above=XY.offset(FOOT_RECESS - 0.01), below=XY.offset(FOOT_RECESS + 0.01), allow_empty=True)
        assert_eq(len(under), 1, msg=kind + ": a recess face 1 mm up inside the foot ring")
        if handle != "none":
            assert_true(hi[0] - lo[0] > diameter + 20.0, msg=kind + ": the handle stands out from the wall")
            roots = b.faces(name="*handle_top_loft*", allow_empty=True)
            assert_true(len(roots) > 0 and len(b.faces(name="*handle_bottom_loft*", allow_empty=True)) > 0, msg=kind + ": both handle roots are blended")

test("colour films sit on the art", decals_sit_on_the_art, sweep={"relief": [0.4, 1.0, 1.6]})
test("foot ring and handle roots", thrown_details, sweep={"handle": ["loop", "d", "none"]})
test("handle roots at every wall", thrown_details, sweep={"wall": [2.4, 2.8, 6.0]})
test("relief volume matches the art", relief_volume, sweep={"diameter": [64, 82, 100], "art_scale": [0.7, 1.0, 1.3]})
test("more art, more relief", relief_volume, sweep={"art_scale": [0.6, 0.8, 1.0, 1.2, 1.4]}, monotone="increasing")
test("mug size and solidity", mug_size, sweep={"diameter": [64, 82, 100], "handle": ["loop", "d", "none"]})

# The default camera, for the trio and for one mug alike: from the art's
# side, so the marks (and the plaque) face the camera and the handles show
# in profile.
def eye_at(az, el, dist, target):
    """The eye `dist` from `target`, `az` degrees from the front toward +x
    and `el` degrees up: the angles `--view az=,el=` takes."""
    return (target[0] + dist * math.sin(az) * math.cos(el), target[1] - dist * math.cos(az) * math.cos(el), target[2] + dist * math.sin(el))

if scene == "trio":
    TRIO_C = (-0.3 * (diameter + 36.0), 0.0, height * 0.45)
    render_defaults(camera=view(eye=eye_at(-30, 20, 5.4 * diameter + 160.0, TRIO_C), target=TRIO_C, fov=30))
else:
    SINGLE_C = (8.0, 0.0, height * 0.48)
    render_defaults(camera=view(eye=eye_at(-24, 16, 3.4 * height, SINGLE_C), target=SINGLE_C, fov=30))

# The site's story: paint, raise, cut. The camera stays near the mark (at
# `art_angle`) so the art is the subject of every step, with the handle in
# profile on the right; it swings off the mark while the relief grows so the
# relief's walls catch the light, and comes back higher for the cut, where
# the light rakes into the pockets.
animation("scroll_story", [
    keyframe(0.0, scene="single", finish="paint", relief=0.4, ease=ease.linear, camera=view(az=-14, el=14, fov=24)),
    keyframe(1.2, finish="paint", relief=0.4, camera=view(az=-24, el=15, fov=24)),
    keyframe(1.25, finish="raised", relief=0.4),
    keyframe(2.6, finish="raised", relief=1.2, camera=view(az=-4, el=18, fov=24)),
    keyframe(2.65, finish="cut", relief=1.2),
    keyframe(3.6, finish="cut", relief=1.0, camera=view(az=-24, el=24, fov=24)),
], fps=20)
