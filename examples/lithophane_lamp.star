# A lithophane night-light: a cylindrical shade whose picture -- a full moon
# rising over three ranges of hills, pines on the nearest, a scatter of
# stars -- lives only in the thickness of its wall. Unlit it is a pale
# cylinder with a faint relief; lit from inside, thin wall glows and thick
# wall stays dark, and the picture comes up out of it.
#
# The picture is procedural: `moonlight(u, v)` below gives the brightness in
# [0, 1] at u mm round the shade and v mm up it, and `lithophane(...)` calls
# it every `picture_pitch` mm, stretches that picture over a grid of
# `resolution` mm and turns brightness into wall (0.6 mm for white,
# 3 mm for black). The bore is a smooth 84 mm cylinder that drops over a
# lamp; a 3 mm band at the foot and the lip is full thickness. It prints
# upright in one piece: every wall is vertical to within a few degrees.
#
#   cadpole eval examples/lithophane_lamp.star
#   cadpole render examples/lithophane_lamp.star -p scene=true --backend path ...
#   cadpole export examples/lithophane_lamp.star -o shade.stl

R = param("bore_radius", 42.0, unit="mm", min=25, max=80, doc="the smooth inside of the shade")
H = param("height", 112.0, unit="mm", min=40, max=200)
res = param("resolution", 0.5, unit="mm", min=0.15, max=2, doc="grid pitch; 0.25 for a print")
pitch = param("picture_pitch", 0.5, unit="mm", min=0.2, max=2, doc="how finely the picture is drawn")
scene = param("scene", False, doc="the shade over its lamp, in a dark room, for rendering")
lit = param("lit", True, doc="with scene=true: the lamp on or off")
power = param("power", 40.0, doc="with scene=true: the lamp's brightness")
ambient = param("ambient", 1.2, doc="with scene=true: the room's faint window light")

THIN = 0.6     # mm of wall for white
THICK = 3.0    # mm of wall for black
FRAME = 3.4    # the solid foot and lip
C = math.tau * R

# ---- noise: integer hashing, so the picture is the same everywhere --------
def hash2(i, j, seed):
    h = (i * 374761393 + j * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 65535.0

def smooth(e0, e1, x):
    t = math.clamp((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)

def vnoise(x, y, seed, period):
    i, j = int(math.floor(x)), int(math.floor(y))
    fx, fy = x - i, y - j
    sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = hash2(i % period, j, seed), hash2((i + 1) % period, j, seed)
    c, d = hash2(i % period, j + 1, seed), hash2((i + 1) % period, j + 1, seed)
    return a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy

def fbm(x, y, seed, octaves, period):
    s, amp, tot = 0.0, 1.0, 0.0
    for o in range(octaves):
        s += amp * vnoise(x, y, seed + o, period * (1 << o))
        tot += amp
        x, y, amp = x * 2, y * 2, amp * 0.5
    return s / tot

# ---- the picture ----------------------------------------------------------
MOON_U, MOON_V, MOON_R = 0.5 * C, 0.66 * H, 13.0    # u = C/2 faces -y, the front

# Three ranges, far to near: (height, relief, seed, bumps per turn, tone).
# Each nearer range is darker, as distance lightens hills.
RANGES = [(0.39 * H, 9.0, 11, 7, 0.50), (0.29 * H, 8.0, 23, 11, 0.26), (0.18 * H, 6.0, 37, 16, 0.04)]

def ridge(u, rng):
    base, amp, seed, freq, _ = rng
    return base + amp * (fbm(u / C * freq, seed * 3.1, seed, 4, freq) - 0.5) * 2

# Everything that depends on u alone is tabled once per 0.25 mm column, so
# the call per node is a few lookups and some arithmetic.
STEP = 0.25
COLS = int(math.ceil(C / STEP))
RIDGES = [[ridge(k * STEP, rng) for k in range(COLS)] for rng in RANGES]

def col(u):
    return int(u / STEP + 0.5) % COLS

# Pines on the nearest ridge: one every 9 mm where the hash allows it.
PINES = []
for k in range(int(C / 9)):
    if hash2(k, 7, 5) >= 0.55:
        tu = k * 9 + 4.5 + (hash2(k, 9, 5) - 0.5) * 5
        th = 9 + 12 * hash2(k, 8, 5)
        PINES.append((tu, RIDGES[2][col(tu)] - 1, th))

# Stars: a fixed scatter above the hills, none in front of the moon, binned
# in 4 mm cells.
STARS = {}
for n in range(200):
    su, sv = hash2(n, 1, 77) * C, 0.36 * H + hash2(n, 2, 77) * 0.59 * H
    du = abs(su - MOON_U)
    if math.hypot(min(du, C - du), sv - MOON_V) > MOON_R + 6:
        key = (int(su // 4), int(sv // 4))
        STARS[key] = STARS.get(key, []) + [(su, sv, 0.55 + 0.75 * hash2(n, 3, 77) * hash2(n, 3, 77) * hash2(n, 3, 77))]

def moonlight(u, v):
    """Brightness at u mm round the shade and v mm up it: 1 white, 0 black."""
    # The pines stand in front of everything.
    for tu, tb, th in PINES:
        if tb < v and v < tb + th and abs(u - tu) < 12:
            half = 0.42 * th * (1 - (v - tb) / th) * (0.85 + 0.15 * math.sin(math.degrees((v - tb) * 2.2)))
            if abs(u - tu) < half + 0.3:
                return 0.0
    # The hills, near to far: below a ridge is that range's tone.
    k = col(u)
    for r in (2, 1, 0):
        h = RIDGES[r][k]
        if v < h:
            return RANGES[r][4] - 0.05 * (1 - v / h)
    # The sky: darker overhead, a glow toward the horizon and round the moon.
    du = abs(u - MOON_U)
    dm = math.hypot(min(du, C - du), v - MOON_V)
    sky = 0.30 + 0.32 * (1 - smooth(0.27 * H, 0.94 * H, v)) + 0.45 * math.exp(-max(0.0, dm - MOON_R) / 16.0)
    b = sky
    if dm < MOON_R + 1.0:
        m = 1.0 - 0.10 * smooth(0.5, 0.8, fbm((du + 50) / 9.0, v / 9.0, 5, 2, 1 << 20)) - 0.08 * math.pow(dm / MOON_R, 4)
        b = sky + (m - sky) * (1 - smooth(MOON_R - 0.6, MOON_R + 0.6, dm))
    ci, cj = int(u // 4), int(v // 4)
    for di in (-1, 0, 1):
        for dj in (-1, 0, 1):
            for su, sv, sr in STARS.get((ci + di, cj + dj), ()):
                d = math.hypot(u - su, v - sv)
                if d < sr + 0.4:
                    b = max(b, 0.95 * (1 - smooth(sr - 0.3, sr + 0.4, d)))
    return math.clamp(b, 0.0, 1.0)

# The lamp is a short tube on the axis, so the wall nearest its middle is lit
# hardest. Part of that fall-off is written back into the thickness -- the
# wall is a little thicker where the light is strongest -- so the picture
# reads evenly from the foot to the lip. FADE is the plastic's effective
# attenuation length (mm) for this correction.
LAMP_Z, FADE = 54.0, 0.3

def glare(v):
    g = math.pow(R * R / (R * R + (v - LAMP_Z) * (v - LAMP_Z)), 1.5)
    g_min = min(math.pow(R * R / (R * R + LAMP_Z * LAMP_Z), 1.5), math.pow(R * R / (R * R + (H - LAMP_Z) * (H - LAMP_Z)), 1.5))
    return FADE * math.log(g / g_min)

EXTRA = glare(LAMP_Z)    # the most the correction adds

def picture(u, v):
    # lithophane() maps 1 to THIN and 0 to THICK + EXTRA; fold the
    # correction into the brightness so both land on the right wall.
    t = THIN + (THICK - THIN) * (1 - moonlight(u, v)) + glare(v)
    return 1 - (t - THIN) / (THICK + EXTRA - THIN)

shade = lithophane(picture, radius=R, height=H, thickness=(THIN, THICK + EXTRA),
                   border=3, border_thickness=FRAME, resolution=res, samples=pitch, name="shade")

# Natural (unpigmented) PLA: milky; light gets through a millimetre or two
# and comes out darker and warmer the thicker the wall.
shade.material(color="#f4ead8", roughness=0.42, transmission=1.0, attenuation=2.0,
               scatter=0.15, scatter_g=0.0, ior=1.46, anisotropy=0.35)

# ---- the scene: the lamp it stands over, in a dark room -------------------
WARM = (1.0, 0.63, 0.32)    # 2700 K, linear RGB, green = 1

if scene:
    e = power if lit else 0.0
    base = cylinder(30.0, 6.0, name="lamp_base")
    base.material(color="#d9d4ca", roughness=0.5)
    stem = cylinder(3.0, LAMP_Z - 28.0, at=(0, 0, 5.0), name="lamp_stem")
    stem.material(color="#c9c4ba", roughness=0.4)
    tube = mesh(field.capsule((0, 0, -20.0), (0, 0, 20.0), 8.0).translate((0, 0, LAMP_Z)), resolution=0.4, name="lamp_tube")
    tube.material(color="#fff4e0", emissive=(WARM[0] * e, WARM[1] * e, WARM[2] * e))
    table = box(1400, 1200, 10, center=True, at=(0, 0, -10), name="table")
    table.material(color="#4a3020", roughness=0.32)
    wall = box(1400, 10, 600, center=True, at=(0, 260, 0), name="wall")
    wall.material(color="#1c1d20", roughness=0.9)
    room = subtract(box(1420, 1220, 620, center=True, at=(0, 0, -20)), box(1400, 1200, 600, center=True, at=(0, 0, -9.99)), name="room")
    room.material(color="#0c0c0d", roughness=1.0)
    window = box(320, 4, 160, center=True, at=(-150, -594, 270), name="window")
    window.material(color="#000000", emissive=(ambient * 0.55, ambient * 0.68, ambient))

export(shade, "lithophane_lamp.stl")

# ---- tests ------------------------------------------------------------------
def closed():
    assert_solid(shade)

def wall_in_range():
    # Bore exactly R; outside never past the frame or the thickest wall.
    lo, hi = shade.bbox()
    out = max(FRAME, THICK + EXTRA)
    assert_true(hi[0] <= R + out + 0.01 and hi[0] >= R + THIN, msg="outer radius %s" % hi[0])
    assert_close(lo[2], 0.0, tol=1e-4)
    assert_close(hi[2], H, tol=1e-4)
    assert_true(shade.volume() > math.pi * ((R + THIN) * (R + THIN) - R * R) * H, msg="thinner than the thinnest wall")
    assert_true(shade.volume() < math.pi * ((R + out) * (R + out) - R * R) * H, msg="thicker than the thickest wall")

def picture_reads():
    # The moon is bright, the near hills black, the frame solid.
    assert_true(moonlight(MOON_U, MOON_V) > 0.9)
    assert_true(moonlight(MOON_U, 2.0) < 0.1)
    assert_true(picture(0.0, 1.0) >= 0.0)

def clears_the_lamp():
    assert_true(R > 30.0 + 5, msg="the bore drops over the lamp's base")

test("the shade is a closed solid", closed)
test("the wall stays between the thinnest and the frame", wall_in_range)
test("the picture reads", picture_reads)
test("the shade drops over its lamp", clears_the_lamp)
