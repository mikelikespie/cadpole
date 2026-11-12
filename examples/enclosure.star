# A printable snap-fit enclosure for a Raspberry Pi 5, built around a model
# of the board. Every opening, standoff and clip is driven from the board's
# own dimensions, so moving a connector in the table below moves its hole.
#
# Dimensional source: Raspberry Pi Ltd, mechanical drawing "Raspberry Pi 5"
# (raspberry-pi-5-mechanical-drawing.pdf, October 2023; the sheet carries
# no revision letter), published at datasheets.raspberrypi.com. The
# drawing is 1:1 on A4 and in vector form, so the parts it shows without a
# dimension were measured off its geometry. Marked in the comments:
#   [drawing]   a dimension printed on the sheet
#   [measured]  measured off the sheet's 1:1 geometry
#   [inferred]  not on the sheet: a typical value for the part, or read from
#               its outline only roughly (heights the side view does not show)
# The drawing says its dimensions are approximate and for reference only;
# the enclosure's clearances are parameters for that reason.
#
# The case is two printed parts. The base is a tray: four standoffs for M2.5
# heat-set inserts (or self-tapping screws), recesses for rubber feet, a
# notch for the microSD card, and a tongue round its rim. The lid is a cover
# that comes down over the board: each connector's opening is a notch open
# at the lid's lower edge, so the lid drops straight on however far the
# connectors stand past the board. The power button is pressed through a
# flexure tab cut in the wall, the status LED shows through a window beside
# it, and the lid carries a grille (or a 30 mm fan) over the SoC, a slot for
# a GPIO ribbon and slots for the two camera and display cables.
#
# The lid clips on. Five flexures are cut in its walls: a strip of wall
# along the lid's lower edge, free at one end, with a hook below it whose
# barb clicks into a pocket in the base's wall. A printed part is weakest
# across its layers, so the flexure lies along them: it runs horizontally
# and bends sideways, in the plane of every layer, and the slit over it
# leans 44 degrees so it prints without support. The flexure tapers in
# thickness toward the hook and in height (the slit's lean), which spreads
# its bend; its root has a fillet; `snap_fit` computes the strain and a test
# holds it to `max_strain`. The hook below it does cross the layers, but it
# is short and stiff and only carries the flexure's force: a second test
# holds its strain to `max_strain_z`. `-p closure=screws` holds the lid with
# four M2.5 countersunk screws instead, up through the base's standoffs and
# the board's holes into pillars in the lid.
#
# A coupon on the hidden `coupon` layer is a short stretch of base and lid
# wall round one clip, as set and a stiffer variant, to print and try by
# hand before the whole case.
#
# Both parts print without support: the base as modelled, the lid upside
# down on its top face. The shell is drawn as tinted, translucent plastic
# so the board shows through it, and the lid is on a layer of its own.
#
#   cadpole test   enclosure.star
#   cadpole check  enclosure.star --layer print --include-hidden --printer bambu_h2d --max-bridge 20
#   cadpole check  enclosure.star --layer coupon --include-hidden --printer bambu_h2d
#   cadpole render enclosure.star --backend path --samples 256 --size 1600x1200 --view az=-35,el=24 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o enclosure_hero.png
#   cadpole render enclosure.star -p explode=18 --view az=-35,el=24 -o enclosure_exploded.png
#   cadpole render enclosure.star -p lid_lift=24 --view az=35,el=28 -o enclosure_open.png
#   cadpole render enclosure.star --layer lid=off -o enclosure_board.png
#   cadpole render enclosure.star -p cooling=fan30 -p gpio=closed -p shell=frost -o variant.png
#   cadpole render enclosure.star -p closure=screws -p lid_lift=24 --view az=35,el=28 -o screws.png
#
# Units are millimetres. The board's centre is the origin in x and y, with
# the USB and Ethernet edge at +x and the GPIO edge at +y; z = 0 is the bed
# under the base.

load("@cadpole/print.star", "drop_to_bed", "snap_fit")

# --- Parameters.
wall = param("wall", 2.8, unit="mm", min=2.4, max=3.6, step=0.1, doc="Wall and floor thickness; each clip's flexure is the outer half of it, less the clearance")
clearance = param("clearance", 0.2, unit="mm", min=0.1, max=0.4, step=0.05, doc="Gap between every sliding face: tongue and rebate, hook and pocket")
board_gap = param("board_gap", 1.0, unit="mm", min=0.6, max=2.0, step=0.1, doc="Gap from the board's edge to the inside of the walls")
headroom = param("headroom", 17.5, unit="mm", min=17, max=30, doc="Inside height above the top of the board (the USB stacks stand 15.8 mm)")
standoff_h = param("standoff_h", 4.5, unit="mm", min=3.0, max=8, step=0.5, doc="Standoff height: the board's underside above the floor (parts on the underside stand about 2 mm)")
closure = param("closure", "clips", choices=["clips", "screws"], doc="How the lid holds on: flexure clips cut in its walls, or four M2.5 countersunk screws up through the base and the board into pillars in the lid")
fastener = param("fastener", "insert", choices=["insert", "self_tap"], doc="What the threaded holes take, M2.5 heat-set inserts or self-tapping screws in a pilot: the standoffs' with clips, the lid pillars' with screws")
corner_r = param("corner_r", 4.0, unit="mm", min=2.5, max=8, doc="Outside corner radius of the case")
edge_break = param("edge_break", 1.0, unit="mm", min=0.4, max=2.0, doc="Break round the lid's top edge and the base's bottom edge, 40 degrees off vertical")
boss_fillet = param("boss_fillet", 1.0, unit="mm", min=0.0, max=2.0, doc="Fillet (a conical flare) where each standoff meets the floor")
port_margin = param("port_margin", 0.5, unit="mm", min=0.2, max=1.5, doc="Clearance round every connector, or its plug, in its opening")
clip_len = param("clip_len", 10.0, unit="mm", min=9, max=12, step=0.5, doc="Clip flexure length, from its root to the middle of its hook: strain falls as its square, and the slit over it rises about as much, so past 10 it needs more headroom")
clip_taper = param("clip_taper", 0.65, min=0.55, max=1.0, step=0.05, doc="Clip flexure thickness at the hook over its thickness at the root")
hook_w = param("hook_w", 8.0, unit="mm", min=5, max=10, doc="Width of each clip's hook")
barb = param("barb", 0.8, unit="mm", min=0.5, max=1.2, step=0.05, doc="How far each hook's barb stands proud of it: it engages barb - clearance, and the flexure bends that far going on")
catch_angle = param("catch_angle", 50.0, unit="deg", min=40, max=60, doc="Catch face from horizontal: under 90 the clip releases with a firm pull; over 45 it prints without support")
lead_angle = param("lead_angle", 30.0, unit="deg", min=20, max=45, doc="Lead-in face from vertical")
max_strain = param("max_strain", 0.01, min=0.005, max=0.03, step=0.001, doc="Permissible bending strain in a clip flexure, which bends along its layers: about 0.01 for PLA used again and again, 0.02 once")
max_strain_z = param("max_strain_z", 0.005, min=0.002, max=0.01, step=0.001, doc="Permissible strain in a part loaded across its layers, the hook as its barb rides over the base: a fraction of the in-layer figure")
cooling = param("cooling", "grille", choices=["grille", "fan30", "none"], doc="Over the SoC: a hex grille, a grille with a 30 mm fan's screw holes (and the fan), or a plain top")
gpio = param("gpio", "slot", choices=["slot", "closed"], doc="A slot in the lid over the GPIO header for a ribbon cable, or a closed top")
ribbons = param("ribbons", True, doc="Slots in the lid over the two camera and display connectors")
button = param("button", "flexure", choices=["flexure", "hole"], doc="Press the power button through a flexure tab cut in the wall, or through an open notch")
foot_d = param("foot_d", 8.0, unit="mm", min=0, max=12, doc="Rubber foot diameter (0: no recesses)")
foot_depth = param("foot_depth", 1.0, unit="mm", min=0.4, max=1.6, doc="Rubber foot recess depth")
shell = param("shell", "smoke", choices=["smoke", "frost", "opaque"], doc="How the shell is drawn: smoked teal or frosted clear translucent plastic, or opaque")
lid_lift = param("lid_lift", 0.0, unit="mm", min=0, max=40, doc="Lift the lid alone by this, the board left seated, so the clips' hooks and the board show; a picture only")
explode_d = param("explode", 0.0, unit="mm", min=0, max=40, doc="Exploded view: lift the board by this and the lid by 2.6 times it; a picture only")

# --- The board.
BL = 85.0  # [drawing]
BW = 56.0  # [drawing]
BOARD_R = 3.0  # [measured]
PCB_T = 1.4  # [measured] 1.41 on the side view
HL = BL / 2
HW = BW / 2


def bxy(bx, by):
    """A point given from the board's -x/-y corner, as the drawing gives it, in model coordinates."""
    return (bx - HL, by - HW)


# Mounting holes: 58 x 49 apart, 3.5 from two edges, 2.7 across in a 6 mm
# keep-out pad. Two further 3 mm holes sit 6 mm in from two of them.
HOLES = [bxy(3.5 + dx, 3.5 + dy) for dx in (0, 58.0) for dy in (0, 49.0)]  # [drawing]
HOLE_D = 2.7  # [drawing]
PAD_D = 6.0  # [drawing]
SMALL_HOLES = [bxy(3.5, 9.5), bxy(61.5, 46.5)]  # [drawing] 3 mm, 6 from the holes

# The connectors that pass through a wall. Each row: name, wall ("front" is
# the -y edge, "right" the +x edge), centre along that edge from the board's
# -x or -y corner, width, height above the board, how far it stands past the
# board's edge, how far it reaches onto the board, and the mating plug's
# body (width, height) where the wall must let a plug in rather than just
# the connector's face (None: the connector's own outline is enough).
PORTS = [
    ("usb_c", "front", 11.2, 8.74, 3.2, 1.28, 6.02, (12.4, 6.6)),  # centre, height [drawing]; width, reach [measured]; plug [inferred]
    ("hdmi0", "front", 25.8, 6.48, 3.4, 1.62, 6.89, (11.0, 6.6)),  # centre, height [drawing]; width, reach [measured]; plug [inferred]
    ("hdmi1", "front", 39.2, 6.48, 3.4, 1.62, 6.89, (11.0, 6.6)),  # as hdmi0
    ("ethernet", "right", 10.2, 15.96, 13.9, 3.0, 18.33, None),  # centre, overhang [drawing]; the rest [measured]
    ("usb3", "right", 29.1, 13.07, 15.8, 2.2, 14.14, None),  # centre [drawing]; the rest [measured]
    ("usb2", "right", 47.0, 13.07, 15.8, 2.2, 14.14, None),  # centre [drawing]; the rest [measured]
]
TALLEST = max([p[4] for p in PORTS])

# On the -x edge: the power button, the status LED, the microSD card (on the
# board's underside) and the PCIe ribbon connector.
BUTTON_Y = 18.4  # [drawing]
BUTTON_BODY = (0.39, 2.93, 16.16, 20.67)  # x0, x1, y0, y1 [measured]
PLUNGER_OUT = 0.45  # [drawing] past the board's edge
PLUNGER_W = 2.0  # [measured]
PLUNGER_Z = 1.6  # [inferred] centre height above the board
BUTTON_H = 3.2  # [inferred]
LED_Y = 13.3  # [drawing]
LED_LEN = 3.2  # [measured]
LED_H = 1.0  # [inferred]
SD_Y = 28.1  # [measured]
SD_W = 11.0  # microSD card width (the card standard)
SD_OUT = 1.72  # [measured] how far the card stands past the board's edge
SD_Z = (-1.2, -0.4)  # [inferred] the card, below the board's underside

# Parts on top of the board, as rectangles from the drawing: x0, x1, y0, y1,
# height above the board.
GPIO = (7.09, 57.83, 49.98, 54.95)  # [measured]; centred 29 from the holes [drawing]
GPIO_BODY_H = 2.5  # [inferred] the header's plastic
GPIO_PIN_H = 8.56  # [measured] pin tips above the board
TOP_PARTS = [
    # name, rect, height, colour, roughness, metallic
    ("soc", (24.56, 41.55, 14.33, 31.27), 1.9, "#b8bdc3", 0.3, 1.0),  # [measured]; height [inferred]
    ("ram", (25.87, 40.35, 34.02, 44.03), 1.1, "#1b1d20", 0.45, 0.0),  # [measured]; height [inferred]
    ("io_chip", (52.33, 64.31, 28.98, 40.96), 1.0, "#1b1d20", 0.45, 0.0),  # [measured]; height [inferred]
    ("shield", (7.19, 17.66, 35.78, 48.86), 1.6, "#c9ced3", 0.3, 1.0),  # [measured]; height [inferred]
    ("csi_dsi_0", (47.18, 50.14, 0.73, 16.19), 4.1, "#2b2d31", 0.5, 0.0),  # [measured]; height [drawing]
    ("csi_dsi_1", (53.39, 56.31, 0.73, 16.19), 4.1, "#2b2d31", 0.5, 0.0),  # [measured]; height [drawing]
    ("rtc_battery", (16.95, 20.97, 3.33, 6.22), 4.4, "#ece6d6", 0.6, 0.0),  # [measured]; height [drawing]
    ("uart", (29.64, 34.96, 2.31, 5.52), 4.4, "#ece6d6", 0.6, 0.0),  # [measured]; height [drawing]
    ("fan_header", (65.15, 68.15, 48.96, 54.95), 4.0, "#ece6d6", 0.6, 0.0),  # [measured]; height [inferred]
    ("pcie", (1.24, 4.20, 24.77, 35.27), 2.0, "#2b2d31", 0.5, 0.0),  # [measured]; height [inferred]
]

# --- The case's sizes.
FLOOR = wall
IN_L = BL + 2 * board_gap
IN_W = BW + 2 * board_gap
OUT_L = IN_L + 2 * wall
OUT_W = IN_W + 2 * wall
IN_R = max(corner_r - wall, 0.6)
PCB_Z = FLOOR + standoff_h  # the board's underside
PCB_TOP = PCB_Z + PCB_T
SPLIT = PCB_Z - 1.0  # where the lid meets the base: below the board, so every connector is in the lid's wall
TONGUE_H = 1.0  # the tongue on the base's rim, into a rebate in the lid
TONGUE_T = (wall - clearance) / 2
TOP_IN = PCB_TOP + headroom  # underside of the lid's top
TOP = TOP_IN + wall
EDGE = {"front": HW, "back": HW, "left": HL, "right": HL}
NORMAL = {"front": (0, -1, 0), "back": (0, 1, 0), "left": (-1, 0, 0), "right": (1, 0, 0)}

# Standoffs. A heat-set insert for M2.5 takes a 3.6 mm hole 4.5 deep; a
# self-tapping M2.5 screw a 2.2 mm pilot. The boss is the pad's 6 mm at the
# top so it stays on the board's keep-out, and flares at the floor. With
# screws, the standoffs take a clearance hole and a countersink from below,
# and the thread is in a pillar that comes down from the lid onto each pad.
BOSS_D = PAD_D
BORE_D = 3.6 if fastener == "insert" else 2.2
SCREWS = closure == "screws"
BORE_DEPTH = 5.0 if fastener == "insert" else (6.0 if SCREWS else standoff_h + 0.6)
CLEAR_D = 2.9  # M2.5 clearance
CSK_D = 5.0  # an M2.5 countersunk head is 4.7 across
CSK_LEAN = 44.0  # the countersink's wall off vertical: a 90-degree head seats on its rim, and it prints
PILLAR_D = PAD_D  # the lid's pillars, on the pads' keep-out

# --- The clips. Each is a flexure in the lid's wall: the wall's outer part
# along the lid's lower edge, from a root to a free end, with a hook under
# its far end that reaches below the split into a recess in the base's
# wall, where a barb on the hook's inside clicks under a lip. In the plane
# of the wall, x runs from the root toward the free end and z is up from
# the split; u is out from the board's edge, as everywhere.
#
# The flexure bends sideways, about a vertical axis, so its bending stress
# runs along it: along the layers. A slit over it frees it from the wall
# above; the slit leans SLIT_LEAN off vertical, so the flexure's underside,
# upside down on the bed, is an overhang that prints, and the flexure is
# taller at its root than at its end.
SNAPS = [("front", 66.0, -1), ("front", 84.0, -1), ("back", 2.0, 1), ("back", 84.0, -1), ("left", 55.0, -1)]  # wall, root's place along it from the board's corner, which way the flexure runs
MODULUS = 3500.0  # N/mm2, PLA's tensile modulus (PETG's is about 2100): the flexure's force
SLIT_LEAN = 44.0
CLIP_SLIT = 0.6  # the slit's width, square to it
CLIP_FILLET = 1.0  # in plan, where the flexure's inner face steps to the wall's
LIP = 1.0  # base wall over each pocket, at the hook's face
FLAT = 0.4  # the barb's flat
U_IN = board_gap + TONGUE_T + clearance  # the lid's rebate face: the flexure's inner face at its root, and the hook's
U_OUT = board_gap + wall
ENGAGE = barb - clearance  # how far a barb reaches under its lip: also how far the flexure bends going on
PLAY = clearance  # how far the lid lifts before a catch face bears


def clip_dims(length, taper, barb_):
    """Every size of one clip, from its length, taper and barb."""
    t0 = U_OUT - U_IN
    t1 = taper * t0
    k = 1.0 / math.tan(SLIT_LEAN)
    engage = barb_ - clearance
    h_tip = TONGUE_H + clearance + 1.0
    x_tip = length + hook_w / 2 + 0.5
    tip_hi = -LIP - clearance - engage * math.tan(catch_angle)
    tip_lo = tip_hi - FLAT
    d = dict(
        length=length, barb=barb_, engage=engage, t0=t0, t1=t1, u_tip=U_OUT - t1, k=k,
        h_tip=h_tip, x_tip=x_tip, h_root=h_tip + x_tip * k, h_hook=h_tip + (x_tip - length) * k,
        slit_v=CLIP_SLIT * math.sqrt(1 + k * k),
        tip_hi=tip_hi, tip_lo=tip_lo, hook_lo=tip_lo - barb_ / math.tan(lead_angle),
        barb_top=tip_hi + barb_ * math.tan(catch_angle),
    )
    # The flexure: its thickness tapers from t0 at the root to t1 at the
    # hook, its height (the slit's lean) from h_root to h_hook. Its force
    # bends the hook across its layers, at its root on the split, from the
    # barb's tip below.
    sf = snap_fit(length, t0, engage, thickness_tip=t1, width=d["h_root"], width_tip=d["h_hook"], modulus=MODULUS)
    d["strain"] = sf["strain"]
    d["factor"] = sf["factor"]
    d["force"] = sf["force"]
    d["hook_strain"] = 6 * sf["force"] * (-tip_lo) / (MODULUS * hook_w * t0 * t0)
    return d


CLIP = clip_dims(clip_len, clip_taper, barb)
STRAIN = CLIP["strain"]
HOOK_STRAIN = CLIP["hook_strain"]


def along_of(side, d):
    """A distance along an edge from the board's corner, as a model coordinate along that edge."""
    return d - HL if side in ("front", "back") else d - HW


def side_box(side, along, width, u0, u1, z0, z1, name=None):
    """A box against one edge of the board: `width` wide centred at `along`,
    from u0 to u1 outward from the board's edge, between z0 and z1."""
    e = EDGE[side]
    kw = {"name": name} if name else {}
    if side == "front":
        return box(width, u1 - u0, z1 - z0, at=(along - width / 2, -(e + u1), z0), **kw)
    if side == "back":
        return box(width, u1 - u0, z1 - z0, at=(along - width / 2, e + u0, z0), **kw)
    if side == "left":
        return box(u1 - u0, width, z1 - z0, at=(-(e + u1), along - width / 2, z0), **kw)
    return box(u1 - u0, width, z1 - z0, at=(e + u0, along - width / 2, z0), **kw)


def side_profile(side, along, width, pts, name=None):
    """A prism `width` wide centred at `along`, whose cross-section is `pts`
    in (outward from the board's edge, z): an arm, a pocket, a nub."""
    n = NORMAL[side]
    t = (n[1], -n[0], 0)  # n x z: so t x n is up and the profile's second axis is z
    e = EDGE[side]
    o = (n[0] * e - t[0] * width / 2 + (along if n[0] == 0 else 0), n[1] * e - t[1] * width / 2 + (along if n[1] == 0 else 0), 0)
    sk = sketch(plane(origin=o, normal=t, x_dir=n)).polygon(pts)
    return extrude(sk, width, **({"name": name} if name else {}))


def wall_slots(side, centres, width, z0, z1, u0, u1, name):
    """Vertical vent slots through a wall, pointed at both ends so their roofs
    print without support either way up."""
    n = NORMAL[side]
    flip = side in ("back", "left")
    a = (1, 0, 0) if side in ("front", "back") else (0, 1, 0)
    xd = (-a[0], -a[1], 0) if flip else a
    o = (n[0] * (EDGE[side] + u0), n[1] * (EDGE[side] + u0), 0)
    sk = sketch(plane(origin=o, normal=n, x_dir=xd))
    hw = width / 2
    pk = 1.8 * hw
    for c in centres:
        s = -c if flip else c
        sk = sk.polygon([(s - hw, z0 + pk), (s, z0), (s + hw, z0 + pk), (s + hw, z1 - pk), (s, z1), (s - hw, z1 - pk)])
    return extrude(sk, u1 - u0, name=name)


def wall_prism(side, pts, u0, u1, name=None):
    """A prism through a wall, from u0 to u1 out from the board's edge, whose
    outline in the wall's plane is `pts`, each (model coordinate along the
    wall, z)."""
    n = NORMAL[side]
    flip = side in ("back", "left")
    a = (1, 0, 0) if side in ("front", "back") else (0, 1, 0)
    xd = (-a[0], -a[1], 0) if flip else a
    o = (n[0] * (EDGE[side] + u0), n[1] * (EDGE[side] + u0), 0)
    sk = sketch(plane(origin=o, normal=n, x_dir=xd)).polygon([(-p[0] if flip else p[0], p[1]) for p in pts])
    return extrude(sk, u1 - u0, **({"name": name} if name else {}))


def plan_xy(side, along, u):
    e = EDGE[side] + u
    return {"front": (along, -e), "back": (along, e), "left": (-e, along), "right": (e, along)}[side]


def plan_prism(side, pts, z0, z1, name=None):
    """A prism from z0 to z1 whose plan is `pts`, each (model coordinate
    along a wall, u out from the board's edge)."""
    sk = sketch(XY.offset(z0)).polygon([plan_xy(side, p[0], p[1]) for p in pts])
    return extrude(sk, z1 - z0, **({"name": name} if name else {}))


def clip_frame(side, root, sense):
    """Model coordinate along the wall of a point x along a clip's flexure."""
    a0 = along_of(side, root)
    return lambda x: a0 + sense * x


def clip_lid_cuts(side, root, sense, d, tag):
    """What a clip takes out of the lid: the slit round its flexure, and the
    inside of the wall behind the flexure, down to its tapered thickness."""
    at = clip_frame(side, root, sense)
    k, s, sv = d["k"], CLIP_SLIT, d["slit_v"]
    xt, ht, hr = d["x_tip"], d["h_tip"], d["h_root"]
    slit = [(xt, -1.0), (xt + s, -1.0), (xt + s, ht + sv - s * k), (0.0, hr + sv), (0.0, hr), (xt, ht)]
    cuts = [wall_prism(side, [(at(x), SPLIT + z) for x, z in slit], board_gap - 1.0, U_OUT + 1.0, name="clip_slit_" + tag)]
    # In plan, the flexure's inner face runs from U_IN at the root, with a
    # fillet into the wall's inside face, to u_tip at the hook.
    r = CLIP_FILLET
    plan = [(0.0, board_gap - 1.0), (0.0, U_IN - r)]
    for i in range(1, 8):
        th = 90.0 * i / 8
        plan.append((r - r * math.cos(th), U_IN - r + r * math.sin(th)))
    xe = xt + s / 2
    plan += [(r, U_IN), (d["length"], d["u_tip"]), (xe, d["u_tip"]), (xe, board_gap - 1.0)]
    behind = plan_prism(side, [(at(x), u) for x, u in plan], SPLIT - 1.0, SPLIT + hr + sv, name="clip_behind_" + tag)
    # ... only under the slit: the wall over it keeps its thickness.
    zone = [(-0.5, -1.5), (xt + s, -1.5), (xt + s, ht + sv / 2 - s * k), (-0.5, hr + sv / 2 + 0.5 * k)]
    cuts.append(intersect(behind, wall_prism(side, [(at(x), SPLIT + z) for x, z in zone], board_gap - 1.5, U_OUT + 1.0), name="clip_thin_" + tag))
    return cuts


def clip_hook(side, root, sense, d, name):
    """A clip's hook: under the flexure's free end, the barb on its inside.
    Its top runs up into the flexure, the step to the flexure's thinner end
    leaning 40 degrees so it prints upside down."""
    at = clip_frame(side, root, sense)
    ch = (d["u_tip"] - U_IN) * 1.2
    prof = [(U_OUT, ch + 0.3), (U_OUT, d["hook_lo"]), (U_IN, d["hook_lo"]), (U_IN - d["barb"], d["tip_lo"]), (U_IN - d["barb"], d["tip_hi"]),
            (U_IN, d["barb_top"]), (U_IN, 0.0), (d["u_tip"], ch), (d["u_tip"], ch + 0.3)]
    return side_profile(side, at(d["length"]), hook_w, [(u, SPLIT + z) for u, z in prof], name=name)


def clip_base_cuts(side, root, sense, d, tag):
    """What a clip takes out of the base: a recess in the wall's outside for
    the hook, and a pocket under a lip for its barb. The pocket's roof is the
    barb's catch face, `clearance` higher."""
    at = clip_frame(side, root, sense)
    c, ub, ta = clearance, U_IN - d["barb"], math.tan(catch_angle)
    w = hook_w + 2 * c
    recess = side_box(side, at(d["length"]), w, U_IN - c, U_OUT + 1.0, SPLIT + d["hook_lo"] - c, SPLIT + TONGUE_H + 1.0, name="clip_recess_" + tag)
    roof = lambda u: d["tip_hi"] + c + (u - ub) * ta
    pocket = [(ub - c, d["tip_lo"] - c), (ub - c, roof(ub - c)), (U_IN, roof(U_IN)), (U_IN, d["hook_lo"] - c)]
    return [recess, side_profile(side, at(d["length"]), w, [(u, SPLIT + z) for u, z in pocket], name="clip_barb_pocket_" + tag)]


def clip_boss(side, root, sense, d, name):
    """The base's wall, thickened inward under the board's edge behind each
    pocket."""
    return side_box(side, clip_frame(side, root, sense)(d["length"]), hook_w + 6.0, 0.0, board_gap + 0.5, FLOOR - 0.5, SPLIT, name=name)


def rr(l, w, r, z0, h, name):
    return extrude(sketch(XY.offset(z0)).rounded_rect(l, w, r), h, name=name)


def rr_ring(lo, wo, ro, li, wi, ri, z0, h, name):
    sk = sketch(XY.offset(z0)).rounded_rect(lo, wo, ro).subtract(sketch(XY.offset(z0)).rounded_rect(li, wi, ri))
    return extrude(sk, h, name=name)


def outline(inset, z):
    return sketch(XY.offset(z)).rounded_rect(OUT_L - 2 * inset, OUT_W - 2 * inset, corner_r - inset)


# ============================================================ the board
pcb_sk = sketch(XY.offset(PCB_Z)).rounded_rect(BL, BW, BOARD_R)
for x, y in HOLES:
    pcb_sk = pcb_sk.circle(HOLE_D / 2, center=(x, y))
for x, y in SMALL_HOLES:
    pcb_sk = pcb_sk.circle(1.5, center=(x, y))
pcb = extrude(pcb_sk, PCB_T, name="pcb")
pcb.material(color="#2b8a4e", roughness=0.45)
board_bodies = [pcb]

pads_sk = sketch(XY.offset(PCB_TOP))
for x, y in HOLES:
    pads_sk = pads_sk.circle(PAD_D / 2 - 0.4, center=(x, y)).circle(HOLE_D / 2 + 0.05, center=(x, y))
pad_bodies = []
for pad in extrude(pads_sk, 0.04, name="pads"):  # one body per ring
    pad.material(color="#d6b25a", roughness=0.3, metallic=1.0)
    pad_bodies.append(pad)
board_bodies += pad_bodies


def rect_part(name, r, h, z0=None):
    (x0, y0), (x1, y1) = bxy(r[0], r[2]), bxy(r[1], r[3])
    return box(x1 - x0, y1 - y0, h, at=(x0, y0, PCB_TOP if z0 == None else z0), name=name)


for name, r, h, col, rough, metal in TOP_PARTS:
    b = rect_part(name, r, h)
    b.material(color=col, roughness=rough, metallic=metal)
    board_bodies.append(b)

# The GPIO header: a black strip and 2 x 20 gilt pins on a 2.54 mm pitch.
gx0, gy0 = bxy(GPIO[0], GPIO[2])
header = rect_part("gpio_strip", GPIO, GPIO_BODY_H)
header.material(color="#1b1d20", roughness=0.55)
gpx = (GPIO[1] - GPIO[0] - 19 * 2.54) / 2
gpy = (GPIO[3] - GPIO[2] - 2.54) / 2
pin = box(0.64, 0.64, GPIO_PIN_H, at=(gx0 + gpx - 0.32, gy0 + gpy - 0.32, PCB_TOP), name="gpio_pin")
pins = pattern_linear(pattern_linear(pin, 20, (2.54, 0, 0), merge=True, name="gpio_row"), 2, (0, 2.54, 0), merge=True, name="gpio_pins")
pins.material(color="#d9b45a", roughness=0.25, metallic=1.0)
board_bodies += [header, pins]

# The power button, the LED and the microSD card on the -x edge.
bb0, bb1 = bxy(BUTTON_BODY[0], BUTTON_BODY[2]), bxy(BUTTON_BODY[1], BUTTON_BODY[3])
btn_body = box(bb1[0] - bb0[0], bb1[1] - bb0[1], BUTTON_H, at=(bb0[0], bb0[1], PCB_TOP), name="button_body")
btn_body.material(color="#c9ced3", roughness=0.3, metallic=1.0)
plunger = side_box("left", along_of("left", BUTTON_Y), PLUNGER_W, -BUTTON_BODY[0] - 0.2, PLUNGER_OUT, PCB_TOP + PLUNGER_Z - 0.8, PCB_TOP + PLUNGER_Z + 0.8, name="button_plunger")
plunger.material(color="#1b1d20", roughness=0.6)
led = side_box("left", along_of("left", LED_Y), LED_LEN, -1.62, 0.0, PCB_TOP, PCB_TOP + LED_H, name="status_led")
led.material(color="#e8f6ee", roughness=0.2, emissive="#1fd36a")
sd_card = side_box("left", along_of("left", SD_Y), SD_W, -12.0, SD_OUT, PCB_Z + SD_Z[0], PCB_Z + SD_Z[1], name="microsd_card")
sd_card.material(color="#26282c", roughness=0.5)
sd_socket = side_box("left", along_of("left", SD_Y), SD_W + 2.0, -14.0, -0.3, PCB_Z - 1.5, PCB_Z, name="microsd_socket")
sd_socket.material(color="#b9bec4", roughness=0.35, metallic=1.0)
board_bodies += [btn_body, plunger, led, sd_card, sd_socket]

# The connectors: a shell from the table, with a mouth in its face and a
# tongue in the mouth.
port_bodies = []
for name, side, at, pw, ph, out, reach, plug in PORTS:
    a = along_of(side, at)
    body = side_box(side, a, pw, -reach, out, PCB_TOP, PCB_TOP + ph)
    mouths = []
    tongues = []
    if name.startswith("usb") and name != "usb_c":
        for zc in (0.27, 0.72):
            mouths.append(side_box(side, a, pw - 1.0, out - 9.0, out + 1, PCB_TOP + ph * zc - 2.7, PCB_TOP + ph * zc + 2.7))
            tongues.append(side_box(side, a, pw - 2.6, out - 9.5, out - 1.2, PCB_TOP + ph * zc - 1.6, PCB_TOP + ph * zc + 0.3))
    elif name == "ethernet":
        mouths.append(side_box(side, a, 11.8, out - 14.0, out + 1, PCB_TOP + 1.0, PCB_TOP + 9.6))
    else:
        mouths.append(side_box(side, a, pw - 0.9, out - 5.5, out + 1, PCB_TOP + 0.45, PCB_TOP + ph - 0.45))
        tongues.append(side_box(side, a, pw - 3.0, out - 6.0, out - 0.6, PCB_TOP + ph * 0.42, PCB_TOP + ph * 0.62))
    b = subtract(body, mouths, name="port_" + name)
    b.material(color="#d5dade", roughness=0.32, metallic=1.0)
    port_bodies.append(b)
    for k, tg in enumerate(tongues):
        tg = translate(tg, (0, 0, 0), name="port_%s_tongue_%d" % (name, k))
        tg.material(color="#2f6fd8" if name == "usb3" else "#1b1d20", roughness=0.5)
        board_bodies.append(tg)
board_bodies += port_bodies

# ============================================================ the base
# The break round the bed edge leans 40 degrees off vertical: a 45-degree one
# would sit exactly on the overhang limit.
BREAK_H = edge_break / math.tan(40.0)
base = loft([outline(edge_break, 0), outline(0, BREAK_H), outline(0, SPLIT)], name="base_outer")
base = subtract(base, rr(IN_L, IN_W, IN_R, FLOOR, SPLIT, name="base_cavity"), name="base_tray")

# The tongue: the inner part of the rim, standing TONGUE_H above the split.
def tongue_ring(grow, name):
    """The rim's tongue, its outside grown by `grow`; the base's own runs from just below the split."""
    t = TONGUE_T + grow
    z0 = SPLIT - 0.5 if grow == 0 and name == "tongue" else SPLIT + 0.05
    return rr_ring(IN_L + 2 * t, IN_W + 2 * t, IN_R + t, IN_L, IN_W, IN_R, z0, SPLIT + TONGUE_H - z0, name)


tongue = tongue_ring(0.0, "tongue")

# Standoffs, flared at the floor.
posts = []
for x, y in HOLES:
    posts.append(cylinder(d=BOSS_D, height=PCB_Z - FLOOR + 0.5, at=(x, y, FLOOR - 0.5)))
    if boss_fillet > 0:
        posts.append(cone(BOSS_D / 2 + boss_fillet, BOSS_D / 2, boss_fillet, at=(x, y, FLOOR - 0.01)))

# Behind each clip's pocket, the base's wall is thickened inward, under the
# board's edge.
bosses = [clip_boss(side, root, sense, CLIP, "clip_boss_%d" % i) for i, (side, root, sense) in enumerate(SNAPS)] if not SCREWS else []
base = union(base, tongue, posts, bosses, kernel="mesh", name="base_built")

if SCREWS:
    # A clearance hole through each standoff, countersunk from below.
    csk_h = (CSK_D - CLEAR_D) / 2 / math.tan(CSK_LEAN)
    bores = [cylinder(d=CLEAR_D, height=PCB_Z + 1, at=(x, y, -0.5)) for x, y in HOLES]
    bores += [cone(CSK_D / 2 + 0.01 * math.tan(CSK_LEAN), CLEAR_D / 2, csk_h + 0.01, at=(x, y, -0.01)) for x, y in HOLES]
else:
    bores = [cylinder(d=BORE_D, height=BORE_DEPTH + 1, at=(x, y, PCB_Z - BORE_DEPTH)) for x, y in HOLES]
feet = []
FEET = [(sx * (OUT_L / 2 - 15.0), sy * (OUT_W / 2 - 8.0)) for sx in (-1, 1) for sy in (-1, 1)]
if foot_d > 0:
    feet = [cylinder(d=foot_d + 0.4, height=foot_depth + 0.5, at=(x, y, -0.5)) for x, y in FEET]

# ============================================================ the openings
# One opening per connector, sized for its plug where it has one, from the
# same table that placed the connector. In the lid each is a notch open at
# the lower edge; one that reaches below the tongue's top also cuts the base.
lid_cuts = []
base_cuts = []


def opening(side, along, width, z_lo, z_hi, name):
    if z_hi > SPLIT:
        lid_cuts.append(side_box(side, along, width, board_gap - 1.0, board_gap + wall + 1.0, SPLIT - 1.0, z_hi, name="cut_" + name))
    if z_lo < SPLIT + TONGUE_H + clearance:
        base_cuts.append(side_box(side, along, width, board_gap - 1.0, board_gap + wall + 1.0, z_lo, SPLIT + TONGUE_H + 1.0, name="notch_" + name))


for name, side, at, pw, ph, out, reach, plug in PORTS:
    zc = PCB_TOP + ph / 2
    w, h = (pw, ph) if plug == None else (max(pw, plug[0]), max(ph, plug[1]))
    opening(side, along_of(side, at), w + 2 * port_margin, zc - h / 2 - port_margin, zc + h / 2 + port_margin, name)


def plug_probes():
    """Each plug's body, 25 mm of it, pushed home against its connector."""
    out = []
    for name, side, at, pw, ph, o, reach, plug in PORTS:
        if plug != None:
            zc = PCB_TOP + ph / 2
            out.append(side_box(side, along_of(side, at), plug[0], o + 0.05, o + 25.0, zc - plug[1] / 2, zc + plug[1] / 2, name="plug_" + name))
    return out

# The microSD card: a notch wide enough for a fingertip each side of it.
opening("left", along_of("left", SD_Y), SD_W + 4.0, PCB_Z + SD_Z[0] - 1.0, PCB_Z + SD_Z[1] + port_margin, "microsd")
# The status LED shows through a window beside the button.
opening("left", along_of("left", LED_Y), LED_LEN + 1.4, PCB_TOP - 0.3, PCB_TOP + LED_H + 1.2, "led")

# The power button: a tab of the lid's wall, free on three sides, with a
# nub that stops `BUTTON_GAP` short of the plunger. Press the tab and the
# nub presses the button. The nub's faces lean 34 degrees off vertical, so
# it prints either way up.
BUTTON_GAP = 0.3
TAB_W = 5.0
TAB_TOP = PCB_TOP + 9.0
SLIT = 0.8
nub = None
if button == "flexure":
    for s in (-1, 1):
        lid_cuts.append(side_box("left", along_of("left", BUTTON_Y) + s * (TAB_W + SLIT) / 2, SLIT, board_gap - 1.0, board_gap + wall + 1.0, SPLIT - 1.0, TAB_TOP, name="button_slit_%d" % (s + 1)))
    zb = PCB_TOP + PLUNGER_Z
    tip = PLUNGER_OUT + BUTTON_GAP
    root = board_gap + 0.3
    nub = side_profile("left", along_of("left", BUTTON_Y), 2.4, [(tip, zb - 0.8), (root, zb - 0.8 - 1.5 * (root - tip)), (root, zb + 0.8 + 1.5 * (root - tip)), (tip, zb + 0.8)], name="button_nub")
else:
    opening("left", along_of("left", BUTTON_Y), 4.5, PCB_TOP - 0.3, PCB_TOP + PLUNGER_Z + 2.0, "button")

if not SCREWS:
    for i, (side, root, sense) in enumerate(SNAPS):
        base_cuts += clip_base_cuts(side, root, sense, CLIP, "%d" % i)
# Each clip's recess runs down the tongue's outside face: the mesh kernel
# takes the shared plane.
base = subtract(base, bores + feet + base_cuts, kernel="mesh", name="base")

# ============================================================ the lid
lid = loft([outline(0, SPLIT), outline(0, TOP - BREAK_H), outline(edge_break, TOP)], name="lid_outer")
cavity = rr(IN_L, IN_W, IN_R, SPLIT - 1.0, TOP_IN - SPLIT + 1.0, name="lid_cavity")
rebate = rr(IN_L + 2 * (TONGUE_T + clearance), IN_W + 2 * (TONGUE_T + clearance), IN_R + TONGUE_T + clearance, SPLIT - 1.0, TONGUE_H + clearance + 1.0, name="rebate")

# The clips: the slit round each flexure, and the wall behind it.
if not SCREWS:
    for i, (side, root, sense) in enumerate(SNAPS):
        lid_cuts += clip_lid_cuts(side, root, sense, CLIP, "%d" % i)

# Vents in the -x wall, above the button and the LED.
VENT_Z0 = PCB_TOP + 6.0
VENT_Z1 = TOP_IN - 2.0
lid_cuts.append(wall_slots("left", [along_of("left", y) for y in (24.5, 28.1, 31.7, 35.3)], 2.0, VENT_Z0, VENT_Z1, board_gap - 1.0, board_gap + wall + 1.0, "side_vents"))

# The top: a hex grille over the SoC (inside a circle, so its edge is
# clean), screw holes for a 30 mm fan, and the ribbon slots.
soc = TOP_PARTS[0][1]
SOC_C = bxy((soc[0] + soc[1]) / 2, (soc[2] + soc[3]) / 2)
GRILLE_R = 13.0
top_sk = sketch(XY.offset(TOP_IN - 1.0))
holes_in_top = 0
if cooling != "none":
    hx, web = 2.6, 1.0
    r = hx / math.sqrt(3.0)
    dx = hx + web
    dy = dx * math.sqrt(3.0) / 2
    n = int(GRILLE_R / dy) + 1
    for j in range(-n, n + 1):
        for i in range(-n - 1, n + 2):
            x = i * dx + (dx / 2 if j % 2 else 0.0)
            y = j * dy
            if math.sqrt(x * x + y * y) + r <= GRILLE_R:
                top_sk = top_sk.ngon(6, r, center=(SOC_C[0] + x, SOC_C[1] + y), rotate=30)
                holes_in_top += 1
if cooling == "fan30":
    for sx in (-1, 1):
        for sy in (-1, 1):
            top_sk = top_sk.circle(1.6, center=(SOC_C[0] + sx * 12.0, SOC_C[1] + sy * 12.0))
            holes_in_top += 1
if gpio == "slot":
    # With screws, the slot's ends stop short of the two pillars beside it.
    sx0, sx1 = GPIO[0] - 1.0, GPIO[1] + 1.0
    if SCREWS:
        sx0, sx1 = max(sx0, 3.5 + PILLAR_D / 2 + 0.1), min(sx1, 61.5 - PILLAR_D / 2 - 0.1)
    g0, g1 = bxy(sx0, GPIO[2] - 1.0), bxy(sx1, GPIO[3] + 0.6)
    top_sk = top_sk.rounded_rect(g1[0] - g0[0], g1[1] - g0[1], 1.0, center=((g0[0] + g1[0]) / 2, (g0[1] + g1[1]) / 2))
    holes_in_top += 1
if ribbons:
    for nm, rct, h, col, rough, metal in TOP_PARTS:
        if nm.startswith("csi_dsi"):
            c0, c1 = bxy(rct[0], rct[2]), bxy(rct[1], rct[3])
            top_sk = top_sk.rounded_rect(2.4, c1[1] - c0[1] + 1.0, 0.6, center=((c0[0] + c1[0]) / 2, (c0[1] + c1[1]) / 2))
            holes_in_top += 1
if holes_in_top > 0:
    lid_cuts.append(extrude(top_sk, wall + 2.0, name="top_openings"))

# A sketch of several separate loops extrudes to a list of bodies.
lid_cuts = [b for c in lid_cuts for b in (c if type(c) == "list" else [c])]
# Some forty cuts meet the shell and each other along short edges: on the
# mesh kernel, which settles them where the exact one does not.
lid = subtract(lid, [cavity, rebate] + lid_cuts, kernel="mesh", name="lid_shell")
# With screws, a pillar from the lid's top down onto each mounting hole's
# pad, bored for the thread. The pillars stand after the top's openings are
# cut, so where one meets the GPIO slot's end it fills that corner.
lid_adds = [nub] if nub != None else []
if SCREWS:
    lid_adds += [cylinder(d=PILLAR_D, height=TOP_IN - PCB_TOP - 0.04 + 0.5, at=(x, y, PCB_TOP + 0.04)) for x, y in HOLES]
else:
    lid_adds += [clip_hook(side, root, sense, CLIP, "clip_hook_%d" % i) for i, (side, root, sense) in enumerate(SNAPS)]
if lid_adds:
    lid = union(lid, lid_adds, kernel="mesh", name="lid_built")
if SCREWS:
    lid = subtract(lid, [cylinder(d=BORE_D, height=BORE_DEPTH + 0.5, at=(x, y, PCB_TOP - 0.5)) for x, y in HOLES], name="lid")
else:
    lid = translate(lid, (0, 0, 0), name="lid")

# The shell's look. `opacity` is what the viewer and the rasterizer blend
# with; the path tracer refracts through `transmission` and tints by
# `color` over `attenuation` mm of plastic, so a wall seen edge-on reads
# darker than one seen square, as printed PETG does.
SHELLS = {
    "smoke": dict(color="#3fc4b4", opacity=0.42, roughness=0.06, ior=1.57, transmission=0.92, attenuation=26.0),
    "frost": dict(color="#e4eef2", opacity=0.38, roughness=0.3, ior=1.57, transmission=0.9, attenuation=40.0),
    "opaque": dict(color="#2f8f88", opacity=1.0, roughness=0.5),
}
base.material(label="base", **SHELLS[shell])
lid.material(label="lid", **SHELLS[shell])

# A 30 mm fan on the grille, for the picture and the fit check.
fan_bodies = []
if cooling == "fan30":
    frame = box(30, 30, 7, center=True, at=(SOC_C[0], SOC_C[1], TOP))
    frame = subtract(frame, cylinder(d=28.5, height=9, at=(SOC_C[0], SOC_C[1], TOP - 1)), name="fan_frame")
    frame.material(color="#2b2d31", roughness=0.6)
    hub = cylinder(d=13, height=6.5, at=(SOC_C[0], SOC_C[1], TOP + 0.25), name="fan_hub")
    hub.material(color="#1b1d20", roughness=0.5)
    fan_bodies = [frame, hub]

# ============================================================ print copies
# What goes on the bed: the base as modelled, the lid turned over onto its
# top. On a hidden layer, so `cadpole check --layer print --include-hidden`
# judges exactly these and the scene never shows them.
printing = layer("print", visible=False, doc="The two printed parts in their print orientation")
base_print = copy(base, name="base_print", layer=printing)
lid_print = translate(rotate(copy(lid), 180, axis=(1, 0, 0), origin=(0, 0, 0)), (0, OUT_W + 10.0, TOP), name="lid_print", layer=printing)

# ============================================================ the coupon
# A short stretch of the base and of the lid round one clip, cut from a
# straight wall with the case's own sections, to print and try by hand
# first: as set, and a stiffer variant (less taper, a deeper barb) that
# holds harder. Each is on the bed as printed, the lid's piece upside down
# on its strip of top; the base pieces carry one dot, then two, in the
# floor, and the lid pieces the same on the inside of their top.
COUPON_VARIANTS = [
    ("as set", clip_dims(clip_len, clip_taper, barb)),
    ("stiffer", clip_dims(clip_len, min(1.0, clip_taper + 0.2), barb + 0.1)),
]
coupon_layer = layer("coupon", visible=False, doc="A clip test coupon: a stretch of base and lid round one clip, as set and stiffer, on the bed")
coupon_fit_layer = layer("coupon_fit", visible=False, doc="The coupon's pieces seated together, where the tests try them")
COUPON_DEPTH = 10.0  # how much floor and top each piece keeps, inside the wall
coupon_bodies = []


def coupon_pieces(i, d):
    """Variant i's base and lid pieces, where a clip on the front wall would
    have them: the root at the stretch's -x end."""
    g = board_gap
    span = d["x_tip"] + CLIP_SLIT
    root = HL - span / 2  # along_of("front", root) = -span / 2
    length = span + 8.0
    mid = 0.0
    base_sec = [(g - COUPON_DEPTH, 0.0), (U_OUT, 0.0), (U_OUT, SPLIT), (g + TONGUE_T, SPLIT), (g + TONGUE_T, SPLIT + TONGUE_H), (g, SPLIT + TONGUE_H), (g, FLOOR), (g - COUPON_DEPTH, FLOOR)]
    lid_sec = [(g - COUPON_DEPTH, TOP_IN), (g, TOP_IN), (g, SPLIT + TONGUE_H + clearance), (U_IN, SPLIT + TONGUE_H + clearance), (U_IN, SPLIT), (U_OUT, SPLIT), (U_OUT, TOP), (g - COUPON_DEPTH, TOP)]
    tag = "coupon%d" % i
    b = side_profile("front", mid, length, base_sec, name=tag + "_base_strip")
    b = union(b, clip_boss("front", root, 1, d, tag + "_boss"), kernel="mesh", name=tag + "_base_built")
    dots = [cylinder(d=1.6, height=1.0, at=(mid - 3.0 + 3.0 * j, -(HW + g - COUPON_DEPTH / 2), FLOOR - 0.6)) for j in range(i + 1)]
    b = subtract(b, clip_base_cuts("front", root, 1, d, tag) + dots, kernel="mesh", name=tag + "_base")
    l = side_profile("front", mid, length, lid_sec, name=tag + "_lid_strip")
    ldots = [cylinder(d=1.6, height=1.0, at=(mid - 3.0 + 3.0 * j, -(HW + g - COUPON_DEPTH / 2), TOP_IN - 0.4)) for j in range(i + 1)]
    l = subtract(l, clip_lid_cuts("front", root, 1, d, tag) + ldots, kernel="mesh", name=tag + "_lid_cut")
    l = union(l, clip_hook("front", root, 1, d, tag + "_hook"), kernel="mesh", name=tag + "_lid")
    return translate(b, (0, 0, 0), name=tag + "_base_seated", layer=coupon_fit_layer), translate(l, (0, 0, 0), name=tag + "_lid_seated", layer=coupon_fit_layer), length


for i, (label, d) in enumerate(COUPON_VARIANTS):
    b, l, length = coupon_pieces(i, d)
    x = i * (length + 8.0)
    # The base piece as modelled; the lid's turned over onto its top, beside it.
    bp = translate(copy(b), (x, HW + board_gap + 2 * COUPON_DEPTH, 0), name="coupon%d_base_print" % i, layer=coupon_layer)
    lp = drop_to_bed(translate(rotate(copy(l), 180, axis=(1, 0, 0), origin=(0, 0, 0)), (x, -(HW + board_gap) + 4.0, 0)), name="coupon%d_lid_print" % i)
    lp = translate(lp, (0, 0, 0), name="coupon%d_lid_on_bed" % i, layer=coupon_layer)
    coupon_bodies.append((label, d, b, l, bp, lp))

# ============================================================ the assembly
# Each part on a layer of its own, so a viewer can switch the lid off.
lid_layer = layer("lid", doc="The lid: switch it off to see the board in the base")
board_layer = layer("board", doc="The board")
base_layer = layer("base", doc="The base")
base_i = instance(part(base, name="base_part"), name="base_i", layer=base_layer)
board_i = instance(part(board_bodies, name="board_part"), name="board_i", layer=board_layer)
lid_i = instance(part([lid] + fan_bodies, name="lid_part"), name="lid_i", layer=lid_layer)
case = assembly([base_i, board_i, lid_i], name="enclosure")
explode(board_i, (0.0, 0.0, explode_d))
explode(lid_i, (0.0, 0.0, 2.6 * explode_d + lid_lift))

report("outside", "%s x %s x %s mm" % (format(OUT_L, ".1f"), format(OUT_W, ".1f"), format(TOP, ".1f")))
if SCREWS:
    report("closure", "4 x M2.5 countersunk, up through the base and the board into the lid's pillars: %s mm of base and board, then %s mm of %s" % (format(PCB_TOP, ".1f"), format(BORE_DEPTH, ".1f"), "heat-set insert" if fastener == "insert" else "self-tapping pilot"))
else:
    report("clips", "%d flexures, %s mm to the hook, %s to %s mm thick; the barb engages %s mm and the flexure bends to %s %% strain along its layers (limit %s %%), with %s N; the hook takes %s %% across its layers (limit %s %%); the lid lifts %s mm before a catch bears" % (
        len(SNAPS), format(clip_len, ".1f"), format(CLIP["t0"], ".2f"), format(CLIP["t1"], ".2f"), format(ENGAGE, ".2f"), format(100 * STRAIN, ".2f"),
        format(100 * max_strain, ".1f"), format(CLIP["force"], ".1f"), format(100 * HOOK_STRAIN, ".2f"), format(100 * max_strain_z, ".2f"), format(PLAY, ".2f")))
for label, d, b, l, bp, lp in coupon_bodies:
    report("coupon " + label, "taper %s, barb %s: the flexure bends to %s %% strain with %s N; the hook %s %%" % (
        format(d["t1"] / d["t0"], ".2f"), format(d["barb"], ".2f"), format(100 * d["strain"], ".2f"), format(d["force"], ".1f"), format(100 * d["hook_strain"], ".2f")))

# ============================================================ tests
TOL = 0.05


def solids():
    assert_solid(base, msg="the base must be a solid")
    assert_solid(lid, msg="the lid must be a solid")
    rb = print_check(base_print, printer="bambu_h2d", max_bridge=20)
    rl = print_check(lid_print, printer="bambu_h2d", max_bridge=20)
    assert_true(rb.components == 1, msg="the base is %s pieces" % rb.components)
    assert_true(rl.components == 1, msg="the lid is %s pieces" % rl.components)


def standoffs():
    # The board sits on the standoffs: touching, not overlapping.
    c = distance(pcb, base)
    assert_true(not c.overlap, msg="the board runs into the base at %s" % (c.at,))
    assert_true(c.distance <= TOL, msg="the board floats %s mm above its standoffs" % c.distance)
    # A screw down each board hole runs into its standoff's bore without
    # touching either: hole and bore share an axis. With screws, one up
    # from under the base runs through the standoff and the board into the
    # lid's pillar.
    for i, (x, y) in enumerate(HOLES):
        if SCREWS:
            screw = cylinder(d=min(HOLE_D, CLEAR_D, BORE_D) - 0.15, height=PCB_TOP + BORE_DEPTH - 0.6, at=(x, y, 0.3), name="screw_probe_%d" % i)
        else:
            screw = cylinder(d=min(HOLE_D, BORE_D) - 0.15, height=BORE_DEPTH + PCB_T + 1.0, at=(x, y, PCB_Z - BORE_DEPTH + 0.3), name="screw_probe_%d" % i)
        assert_true(distance(screw, pcb).distance > 0.0, msg="hole %d: the screw touches the board" % i)
        assert_true(distance(screw, base).distance > 0.0, msg="hole %d: the screw misses the standoff's bore" % i)
        if SCREWS:
            assert_true(distance(screw, lid).distance > 0.0, msg="hole %d: the screw misses the pillar's bore" % i)


def connectors_clear():
    # Every connector clears its opening by the margin, and every plug gets in.
    for b in port_bodies:
        for shell_part in (lid, base):
            d = distance(b, shell_part)
            assert_true(d.distance >= port_margin - 2 * d.tolerance - 0.01, msg="%s is %s mm from %s at %s" % (b, d.distance, shell_part, d.at))
    for p in plug_probes():
        for shell_part in (lid, base):
            d = distance(p, shell_part)
            assert_true(not d.overlap and d.distance > 0.1, msg="%s hits %s at %s" % (p, shell_part, d.at))
    d = distance(sd_card, base)
    assert_true(not d.overlap and d.distance > 0.3, msg="the microSD card hits the base at %s" % (d.at,))


def board_clear_of_lid():
    assert_true(PCB_TOP + TALLEST < TOP_IN - 1.0, msg="the USB stacks hit the lid: raise headroom")
    for b in board_bodies:
        if b == plunger or (SCREWS and (b == pcb or b in pad_bodies)):
            continue
        d = distance(b, lid)
        assert_true(d.distance > 0.2, msg="%s touches the lid at %s" % (b, d.at))
    if nub != None:
        d = distance(plunger, nub)
        assert_true(math.close(d.distance, BUTTON_GAP, tol=0.02), msg="the button's nub is %s mm off the plunger" % d.distance)
    if SCREWS:
        # The pillars clamp the board on its pads.
        for p in pad_bodies:
            d = distance(p, lid)
            assert_true(not d.overlap and d.distance <= TOL, msg="a pillar stands %s mm off its pad" % d.distance)


def lid_fits_base():
    d = distance(lid, base)
    assert_true(not d.overlap, msg="the lid overlaps the base at %s" % (d.at,))
    assert_true(d.distance <= TOL, msg="the lid does not sit on the base: %s mm" % d.distance)
    # The tongue runs in the lid's rebate `clearance` off it all round, and
    # a tongue any wider would bear on the lid: so the lid is located to
    # within the clearance in every direction.
    t = tongue_ring(0.0, "tongue_probe")
    d = distance(t, lid)
    assert_true(not d.overlap and math.close(d.distance, clearance, tol=2 * d.tolerance + 0.01), msg="the tongue runs %s mm off the lid at %s" % (d.distance, d.at))
    assert_true(distance(tongue_ring(clearance + 0.1, "tongue_wide"), lid).distance == 0.0, msg="a wider tongue would still not bear on the lid")


def clips_engage():
    if SCREWS:
        return TOP
    assert_true(ENGAGE >= 0.4, msg="the barbs engage only %s mm" % ENGAGE)
    assert_true(STRAIN <= max_strain, msg="the flexures bend to %s strain, over %s" % (STRAIN, max_strain))
    assert_true(HOOK_STRAIN <= max_strain_z, msg="the hooks take %s strain across their layers, over %s" % (HOOK_STRAIN, max_strain_z))
    assert_true(CLIP_FILLET >= 0.5 * CLIP["t0"], msg="the flexure's root fillet is under half its thickness")
    assert_true(CLIP["t1"] >= 0.7, msg="the flexure ends %s mm thick: under a perimeter and three quarters" % CLIP["t1"])
    assert_true(CLIP["h_root"] + CLIP["slit_v"] <= TOP_IN - SPLIT - 1.0, msg="the slit runs into the lid's top")
    assert_true(SPLIT + CLIP["hook_lo"] - clearance >= 0.4, msg="the hooks' recesses reach the bed")
    assert_true(U_IN - barb - clearance >= 0.8, msg="the base keeps under two perimeters behind a pocket")
    # Seated, every hook clears the base. Lifted past the play and the catch
    # face's rise, every barb bears on its lip. Moved out by just under the
    # stated engagement it still bears; by just over, it lets go: the
    # interference is `barb - clearance`, measured on the geometry.
    lifted = translate(copy(base), (0, 0, -(PLAY + ENGAGE * math.tan(catch_angle) + 0.3)), name="base_dropped")
    for i, (side, root, sense) in enumerate(SNAPS):
        h = clip_hook(side, root, sense, CLIP, "hook_probe_%d" % i)
        d = distance(h, base)
        assert_true(not d.overlap and d.distance >= clearance - 2 * d.tolerance - 0.01, msg="clip %d rubs the base: %s mm at %s" % (i, d.distance, d.at))
        assert_true(distance(h, lifted).overlap, msg="clip %d does not catch the base" % i)
        n = NORMAL[side]
        for e, held in [(ENGAGE - 0.08, True), (ENGAGE + 0.08, False)]:
            moved = translate(copy(h), (n[0] * e, n[1] * e, 0))
            assert_true(distance(moved, lifted).overlap == held, msg="clip %d moved out %s mm: held %s, expected %s" % (i, e, not held, held))
    return TOP


def coupon_prints():
    for label, d, b, l, bp, lp in coupon_bodies:
        for piece in (bp, lp):
            r = print_check(piece, printer="bambu_h2c")
            assert_true(r.solid and r.components == 1, msg="the %s coupon's %s is not one solid" % (label, piece))
            assert_true(r.unsupported_area == 0.0, msg="the %s coupon's %s needs support: %s mm2" % (label, piece, r.unsupported_area))
            assert_support_free(piece, printer="bambu_h2c")
        lo, hi = lp.bbox()
        assert_close(lo[2], 0.0, tol=1e-4)
        # Seated, the stretch of lid sits on the stretch of base and its hook
        # clears; lifted, the barb bears.
        dd = distance(l, b)
        assert_true(not dd.overlap and dd.distance <= TOL, msg="the %s coupon's pieces do not seat: %s" % (label, dd.distance))
        up = translate(copy(l), (0, 0, PLAY + d["engage"] * math.tan(catch_angle) + 0.3))
        assert_true(distance(up, b).overlap, msg="the %s coupon does not catch" % label)
    assert_true(COUPON_VARIANTS[1][1]["force"] > COUPON_VARIANTS[0][1]["force"], msg="the stiffer variant is not stiffer")


def printable():
    # The bridges are the feet recesses' roofs; every other face stands at
    # 45 degrees or steeper, the slits over the clips' flexures at 46.
    rb = print_check(base_print, printer="bambu_h2d", max_bridge=20)
    rl = print_check(lid_print, printer="bambu_h2d", max_bridge=20)
    assert_true(rb.solid and rl.solid, msg="both parts must be closed solids")
    assert_true(rb.unsupported_area == 0.0, msg="the base needs support: %s mm2" % rb.unsupported_area)
    assert_true(rl.unsupported_area == 0.0, msg="the lid needs support: %s mm2" % rl.unsupported_area)
    assert_true(rb.min_wall >= 0.79 and rl.min_wall >= 0.79, msg="walls %s and %s" % (rb.min_wall, rl.min_wall))
    assert_true(rb.fits_bed and rl.fits_bed)
    # The support rules too: no ledge, island or sagging hole, the clips'
    # hooks and the lid's pillars included.
    assert_support_free(base_print, printer="bambu_h2d", max_bridge=20)
    assert_support_free(lid_print, printer="bambu_h2d", max_bridge=20)


test("base and lid are each one solid", solids)
test("the board sits on its standoffs and every hole lines up", standoffs)
test("every connector clears its opening and every plug gets in", connectors_clear)
test("nothing on the board touches the lid", board_clear_of_lid)
test("the lid seats on the base and the tongue locates it", lid_fits_base)
test("the clips engage by barb minus clearance, within their strain limits", clips_engage)
test("the clips engage at every height, and taller cases are taller", clips_engage, sweep={"headroom": [17, 20, 25, 30]}, monotone="increasing")
test("the clips engage across their parameters", clips_engage, sweep={"barb": [0.7, 0.9], "clearance": [0.15, 0.25]})
test("the clips fit and hold across length, taper and wall", clips_engage, sweep={"clip_len": [10, 12], "headroom": [20], "clip_taper": [0.65, 0.8], "wall": [2.6, 3.0]})
test("the coupon's pieces print, seat and catch, and the stiffer one is stiffer", coupon_prints)
test("standoffs fit either fastener", standoffs, sweep={"fastener": ["insert", "self_tap"], "standoff_h": [3.0, 6.0]})
test("everything clears across wall and gap", connectors_clear, sweep={"wall": [2.4, 3.4], "board_gap": [0.6, 1.6]})
test("every top and button option builds and clears", board_clear_of_lid, sweep={"cooling": ["grille", "fan30", "none"], "gpio": ["slot", "closed"], "button": ["flexure", "hole"]})
test("prints without support", printable)
test("with screws: each screw runs through base and board into its pillar, which clamps the board", standoffs, sweep={"closure": ["screws"], "fastener": ["insert", "self_tap"]})
test("with screws: nothing else on the board touches the lid", board_clear_of_lid, sweep={"closure": ["screws"]})
test("with screws: one solid each, and no support", printable, sweep={"closure": ["screws"]})
