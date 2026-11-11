# Cadpole, the mascot, as a printable desk figure: a round green tadpole
# with two big eyes and a wiggly tail drawn the way the logo draws it -- a
# spline -- sitting on a plinth that says "cadpole". With `handles=true`,
# as a CAD wink, the tail's Bezier handles stand over it on stalks: a bar
# along the tangent and a diamond knob at each handle end. They are off by
# default: at desk distance they read as pins stuck in the tail.
#
# How it is built:
#   * The figure is one signed-distance field: the head is an egg (a big
#     sphere with a smaller one `smooth_union`ed on behind it), the tail a
#     chain of capsules along a wiggling spline (amplitude and waves are
#     parameters), tapering to a fifth of its thickness, and the two are
#     blended so the neck is a real fillet and the tip a round. It is meshed
#     once with `mesh()` and cut flat on the plinth's top: the figure stands
#     on its own footprint, and the tail's section sits high enough on the
#     plinth that its flanks underneath stay within 45 degrees.
#   * The eyes are exact spheres; the body is cut where the whites sit and
#     each white where its pupil sits, so the colours are separate bodies
#     that touch and never overlap -- one object of several parts in a
#     multi-colour slicer.
#   * The handles' knobs are bicones: 45-degree flanks, self-supporting.
#   * The plinth is an exact B-rep (a filleted stadium). The word is raised
#     on its top, centred on the straight stretch between the round ends and
#     shrunk to fit it, so any `word` stays on the flat.
#
#   cadpole test desk_mascot.star
#   cadpole check desk_mascot.star
#   cadpole render desk_mascot.star --backend path --samples 256 --size 1600x1200 --view az=-35,el=24 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o desk_mascot.png

size = param("size", 110.0, unit="mm", min=60, max=220, doc="Length of the plinth; everything scales with it")
wiggle = param("wiggle", 0.8, min=0.0, max=1.2, doc="Tail wiggle amplitude, as a fraction of the body radius")
waves = param("waves", 1.5, min=0.5, max=2.5, doc="Tail wiggle frequency: full waves along the tail")
handles = param("handles", False, doc="Stand the tail's Bezier handles over it on stalks, a CAD joke that reads best up close")
word = param("word", "cadpole", doc="What the plinth says")
fused_check = param("fused", False, doc="Also build the fused print, on a hidden layer, for `cadpole check --layer fused`")

GREEN = "#14967F"
DARK = "#0E6E5E"
WHITE = "#F4F6F2"
INK = "#0B2E2A"
PLINTH = "#3B4A50"  # slate: the green figure stands off it, the white word reads on it, and it is light enough for a dark page

S = size / 110.0  # everything is drawn at size 110 and scaled by this
R = 17.0 * S  # body radius
BASE_L = size
BASE_W = 50.0 * S
BASE_H = 11.0 * S
TOP = BASE_H

# The head: an egg, a big sphere with a smaller one blended on behind it, its
# centre high enough that the cut on the plinth leaves 45-degree flanks.
BX = -BASE_L / 2.0 + R + 9.0 * S  # body centre
BY = 3.0 * S
BZ = TOP + 0.66 * R


def v_add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def v_sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def v_mul(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def v_len(a):
    return math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])


def v_unit(a):
    l = v_len(a)
    return (a[0] / l, a[1] / l, a[2] / l)


def v_cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


# --- The centre line: s runs 0..1 from inside the head to the tail's tip.
# It runs along the plinth and wiggles sideways with an amplitude that grows
# toward the tip.
X0 = BX + 0.35 * R  # the tail starts inside the head, low at its back
X1 = BASE_L / 2.0 - 6.0 * S
S_WIG = 0.16  # where the wiggle starts
WAVELENGTH = (X1 - X0) * (1.0 - S_WIG) / waves
# A swing wider than this brings the tail's neighbouring turns together, so
# they fuse into one lump and the curve no longer reads; the amplitude is held to it.
AMP = min(wiggle * R, 0.45 * WAVELENGTH)
if AMP < wiggle * R:
    warn("wiggle %s is held to %s mm: %s mm would crowd the tail's turns together at %s waves" % (wiggle, math.round(AMP, 1), math.round(wiggle * R, 1), waves))


def taper(s):
    return 1.0 - 0.80 * math.pow(s, 0.85)


def radius(s):
    """The tail's round section at s: fleshy at the neck, a fifth of that at the tip."""
    return 0.40 * R * taper(s)


# The section's centre sits this fraction of its radius above the plinth: the
# cut leaves flanks no steeper than 45 degrees underneath, so it prints
# without support, and more than half the round shows.
LIFT = 0.6


def centre(s):
    q = math.clamp((s - S_WIG) / (1.0 - S_WIG), 0.0, 1.0)
    y = BY + AMP * math.pow(q, 0.7) * math.sin(360.0 * waves * q)
    return (X0 + (X1 - X0) * s, y, TOP + LIFT * radius(s))


def tangent(s):
    e = 0.002
    return v_unit(v_sub(centre(min(1.0, s + e)), centre(max(0.0, s - e))))


spine = curve(centre(0.0), name="spine").spline([centre(k / 40.0) for k in range(1, 41)])

# One signed-distance field for the whole figure: the head is an egg (a
# smaller sphere blended on behind a big one); the tail is a chain of short
# cone frustums along the centre line, each a little thinner than the last,
# with a ball at every joint; and the tail is blended onto the head, so the neck is a
# real fillet rather than a crease and the tip ends in a round. The field is
# meshed once and cut flat on the plinth's top.
NECK = 0.35 * R  # the blend radius where the tail grows out of the head
N_SEG = int(12 + 8 * waves)  # short enough to follow the tightest turn
egg = field.sphere(R).translate((BX, BY, BZ)).smooth_union(field.sphere(0.72 * R).translate((BX + 0.45 * R, BY, BZ - 0.18 * R)), 0.4 * R)


def round_cone(a, b, ra, rb):
    """A cone frustum from a (radius ra) to b (radius rb) with a ball on b: one
    link of a tube whose radius changes smoothly, with no seam at the joints."""
    d = v_sub(b, a)
    l = v_len(d)
    u = v_mul(d, 1.0 / l)
    cone = field.cone(ra, rb, l).rotate(math.acos(math.clamp(u[2], -1.0, 1.0)), axis=v_cross((0, 0, 1), u)).translate(v_mul(v_add(a, b), 0.5))
    return cone.union(field.sphere(rb).translate(b))


tail = field.sphere(radius(0.0)).translate(centre(0.0))
for k in range(N_SEG):
    s0 = k / float(N_SEG)
    s1 = (k + 1) / float(N_SEG)
    tail = tail.union(round_cone(centre(s0), centre(s1), radius(s0), radius(s1)))
whole = mesh(egg.smooth_union(tail, NECK), resolution=0.3 * S, name="tadpole_whole")
# The flat foot is a mesh boolean, not a half-space in the field, so the
# plane comes out flat. It sits 5 microns above the plinth's top: the meshes
# are stored in single precision, and a foot cut exactly at the top can dip a
# fraction of a micron into it, which reads as two colours overlapping.
FOOT = TOP + 0.005
figure = subtract(whole, box(4.0 * BASE_L, 4.0 * BASE_L, 100.0 * S, at=(-2.0 * BASE_L, -2.0 * BASE_L, FOOT - 100.0 * S)), name="figure", kernel="mesh")

# --- Eyes: two big spheres on the brow, looking forward and a little out.
EYE_R = 0.34 * R
PUPIL_R = 0.19 * R


GAZE = v_unit((-0.55, -0.50, 0.62))  # where the face looks: front-left and up, toward the hero camera
EYE_SPREAD = v_unit((-0.50, 0.55, 0.0))  # across the face, horizontal and square to the gaze


def eye_dir(side):
    """The two eyes sit either side of the gaze, so from the front-left they read as a pair."""
    return v_unit(v_add(GAZE, v_mul(EYE_SPREAD, side * 0.52)))


def look_dir(side):
    return v_unit(v_add((-0.30, -0.80, 0.52), v_mul(EYE_SPREAD, -side * 0.08)))  # at the viewer, a touch cross-eyed


GAP = 0.04  # each colour's socket is this much bigger than the part in it: they touch, never overlap
eye_whites = []
eye_pupils = []
eye_glints = []
sockets = []
for k, side in enumerate((1.0, -1.0)):
    c = v_add((BX, BY, BZ), v_mul(eye_dir(side), 0.80 * R))
    pc = v_add(c, v_mul(look_dir(side), EYE_R - 0.55 * PUPIL_R))
    gc = v_add(pc, v_mul(v_unit(v_add(look_dir(side), (0.1, 0.0, 0.9))), PUPIL_R * 0.78))
    GR = 0.055 * R
    sockets.append(sphere(EYE_R + GAP, center=c, name="eye_socket_%d" % k))
    white = subtract(sphere(EYE_R, center=c, name="eye_ball_%d" % k), sphere(PUPIL_R + GAP, center=pc, name="pupil_socket_%d" % k), name="eye_%d" % k, kernel="mesh")
    pupil = subtract(sphere(PUPIL_R, center=pc, name="pupil_ball_%d" % k), sphere(GR + GAP, center=gc, name="glint_socket_%d" % k), name="iris_%d" % k, kernel="mesh")
    glint = intersect(sphere(GR, center=gc, name="glint_ball_%d" % k), sphere(PUPIL_R, center=pc, name="glint_clip_%d" % k), name="spark_%d" % k, kernel="mesh")
    eye_whites.append(white)
    eye_pupils.append(pupil)
    eye_glints.append(glint)
body = subtract(figure, sockets[0], sockets[1], name="cadpole_body", kernel="mesh")


# --- The plinth: a filleted stadium with the word on its front face.
base_sk = sketch(XY).slot(BASE_L, BASE_W)
base = extrude(base_sk, BASE_H, name="plinth")
base = fillet(base, radius=2.0 * S, edges=base.edges(on_plane=XY.offset(BASE_H)), name="plinth_round")
base.material(preset="pla_matte", color=PLINTH, roughness=0.6)
WORD_RELIEF = 1.0 * S
# The word lies on the plinth's top, in front of the tail, so it prints flat
# (a colour change at one layer height does it on a one-nozzle printer). It
# is centred on the straight stretch of the stadium, between the two round
# ends, and kept a margin inside the top's fillet: set once at full size,
# measured, then shrunk if it is longer than that stretch and set again
# with its lowest point the margin above the front edge.
ROUND = 2.0 * S  # the top edge's fillet
WORD_MARGIN = 2.5 * S
FLAT_HALF = BASE_L / 2.0 - BASE_W / 2.0  # half the straight stretch of the stadium
word_plane = plane(origin=(0.0, 0.0, TOP), normal=(0, 0, 1), x_dir=(1, 0, 0))


def word_extent(sk):
    """(min x, max x, min y, max y) of a sketch's loops, in its plane."""
    xs = [p[0] for loop in sk.points() for p in loop]
    ys = [p[1] for loop in sk.points() for p in loop]
    return (min(xs), max(xs), min(ys), max(ys))


def set_word(size, at, name):
    return text(word, size=size, weight=700, align="center", baseline="middle", spacing=0.6 * size / 9.0, plane=word_plane, at=at, name=name)


WORD_SIZE = 9.0 * S
(wx0, wx1, wy0, wy1) = word_extent(set_word(WORD_SIZE, (0.0, 0.0), "word_measure"))
WORD_SIZE = WORD_SIZE * min(1.0, 2.0 * (FLAT_HALF - WORD_MARGIN) / (wx1 - wx0))
(wx0, wx1, wy0, wy1) = word_extent(set_word(WORD_SIZE, (0.0, 0.0), "word_fitted"))
WORD_X = -0.5 * (wx0 + wx1)
WORD_Y = -BASE_W / 2.0 + ROUND + WORD_MARGIN - wy0
label = set_word(WORD_SIZE, (WORD_X, WORD_Y), "word")
letters = extrude(label, WORD_RELIEF, name="word_relief")
letters = letters if type(letters) == "list" else [letters]


# --- The CAD wink: the tail's Bezier handles, standing over the spine.
LINE_R = 0.6 * S  # the handle's line: 1.2 mm across at the default size
HANDLE_LEN = 0.85 * R
HANDLE_RISE = 52.0  # degrees above horizontal: steeper than 45 prints without support
KNOB = 1.7 * S


def bicone(c, r, name, turn=45.0):
    """An octahedron of half-diagonal r about c: a CAD handle's diamond knob,
    every face at 45 degrees, so it prints without support."""
    sq = [(c[0] + r * math.cos(turn + 90.0 * k), c[1] + r * math.sin(turn + 90.0 * k)) for k in range(4)]
    mid = sketch(XY.offset(c[2])).polygon(sq)
    return loft([(c[0], c[1], c[2] - r), mid, (c[0], c[1], c[2] + r)], name=name)


handle_parts = []
handle_knobs = []
handle_raw = []
NODE_S = []
if handles:
    # A node at every crest of the wiggle and one handle from it, the way the
    # logo draws its Bezier handles: a thin line along the tangent (leaning
    # up at HANDLE_RISE, so it prints without support) to a small diamond.
    for k in range(0, 12):
        q = (0.25 + k * 0.5) / waves
        if q > 0.05 and q < 0.92:
            NODE_S.append(S_WIG + q * (1.0 - S_WIG))
    if len(NODE_S) == 0:
        NODE_S = [0.6]
    for i, s in enumerate(NODE_S):
        p = centre(s)
        t = tangent(s)
        sgn = 1.0 if i % 2 == 0 else -1.0  # alternate: forward from one crest, back from the next
        th = v_mul(v_unit((t[0], t[1], 0.0)), sgn)
        root = (p[0], p[1], p[2] + 0.80 * radius(s))  # just inside the tail's crest; trimmed to its skin below
        d = v_add(v_mul(th, math.cos(HANDLE_RISE)), (0.0, 0.0, math.sin(HANDLE_RISE)))
        tip = v_add(root, v_mul(d, HANDLE_LEN))
        line = extrude(sketch(plane(origin=root, normal=d, x_dir=v_unit(v_cross(d, (0, 0, 1))))).circle(LINE_R), HANDLE_LEN, name="handle_line_%d" % i)
        knob = bicone(tip, KNOB, "knob_%d" % i, turn=math.atan2(th[1], th[0]) + 45.0)
        if fused_check:
            handle_raw.append(union(copy(line), copy(knob), name="handle_raw_%d" % i, kernel="mesh"))
        # as in the logo: a dark green line, a white knob; two colours that touch
        line = subtract(line, copy(figure, name="skin_clip_%d" % i), name="handle_%d" % i, kernel="mesh")
        socket = extrude(sketch(plane(origin=root, normal=d, x_dir=v_unit(v_cross(d, (0, 0, 1))))).circle(LINE_R + GAP), HANDLE_LEN, name="line_socket_%d" % i)
        knob = subtract(knob, socket, name="knob_cut_%d" % i, kernel="mesh")
        handle_parts.append(line)
        handle_knobs.append(knob)

# --- Materials.
body.material(preset="pla_silk", color=GREEN, roughness=0.32)
for e in eye_whites:
    e.material(preset="pla_matte", color=WHITE, roughness=0.35)
for p in eye_pupils:
    p.material(preset="petg_glossy", color=INK, roughness=0.12)
for g in eye_glints:
    g.material(preset="pla_matte", color=WHITE)
for h in handle_parts:
    h.material(preset="pla_matte", color=DARK, roughness=0.4)
for h in handle_knobs:
    h.material(preset="pla_matte", color=WHITE, roughness=0.4)

for g in letters:
    g.material(preset="pla_matte", color=WHITE, roughness=0.4)

PRINT = [body, base] + letters + eye_whites + eye_pupils + eye_glints + handle_parts + handle_knobs
export(PRINT, "desk_mascot.3mf")

# The parts as the slicer prints them, fused: one object of several colours.
# `cadpole check` judges each colour part on its own, where an eye's lower half
# or a stalk's foot looks like it hangs in the air; in the print the body and
# the tail hold them up. `cadpole check -p fused=true --layer fused` judges this.
if fused_check:
    fused_layer = layer("fused", doc="Every part fused, as printed", visible=False)
    balls = [sphere(EYE_R, center=v_add((BX, BY, BZ), v_mul(eye_dir(sd), 0.80 * R)), name="fused_eye_%d" % k) for k, sd in enumerate((1.0, -1.0))]
    plinth_word = union([copy(base, name="fused_plinth")] + [translate(copy(g), (0, 0, -0.2)) for g in letters], name="fused_plinth_word", kernel="mesh")
    fused = union([plinth_word, translate(copy(figure, name="fused_figure"), (0, 0, -0.2))] + balls + handle_raw, name="fused_print", kernel="mesh", layer=fused_layer)


report("tail_length_mm", spine.length)
report("handles", len(handle_parts))


# --- Tests.
def every_part_is_a_solid():
    for b in PRINT:
        assert_solid(b, msg="%s is not a solid" % b.color())
    return len(PRINT)


test("every part is a solid", every_part_is_a_solid, sweep={"wiggle": [0.0, 0.4, 0.8, 1.2], "waves": [0.5, 1.5, 2.5]})


def stands_on_the_plinth():
    (lo, hi) = body.bbox()
    assert_close(lo[2], TOP, tol=0.3, msg="the figure's foot is on the plinth")
    assert_true(hi[0] < BASE_L / 2.0 and lo[0] > -BASE_L / 2.0, msg="the figure stays on the plinth along x")
    assert_true(hi[1] < BASE_W / 2.0 and lo[1] > -BASE_W / 2.0, msg="... and across it")
    return hi[2]


test("the figure stands on the plinth, inside its outline", stands_on_the_plinth, sweep={"wiggle": [0.0, 0.8, 1.2], "waves": [0.5, 1.0, 2.0, 2.5]})


def scales_with_size():
    (lo, hi) = base.bbox()
    assert_close(hi[0] - lo[0], size, rel=0.01)
    return body.volume()


test("everything scales with size", scales_with_size, sweep={"size": [70.0, 110.0, 160.0]}, monotone="increasing")


def colours_do_not_overlap():
    for a, b in [(body, eye_whites[0]), (body, eye_whites[1]), (eye_whites[0], eye_pupils[0]), (eye_pupils[0], eye_glints[0]), (body, base), (base, letters[0])]:
        c = distance(a, b)
        assert_true(not c.overlap, msg="%s and %s overlap" % (a.color(), b.color()))
    return 0


test("the colour parts touch and never overlap", colours_do_not_overlap)


def word_sits_on_the_top():
    flat = sketch(word_plane).slot(BASE_L - 2.0 * ROUND, BASE_W - 2.0 * ROUND)
    off = label.subtract(flat).area()
    assert_true(off < 1e-6, msg="%s mm^2 of the word is off the plinth's flat top" % off)
    (x0, x1, y0, y1) = word_extent(label)
    assert_true(x0 > -FLAT_HALF and x1 < FLAT_HALF, msg="the word runs onto the plinth's round ends")
    for b in letters:
        assert_true(not distance(b, body).overlap, msg="the word runs into the figure")
    return label.area()


test("the word sits on the plinth's flat top", word_sits_on_the_top, sweep={"size": [70.0, 110.0, 160.0], "word": ["cadpole", "tadpoles of the desk"]})
