"""@cadpole/plan_furniture.star -- the room stage's catalog of furniture and fixtures, as data.

    load("@cadpole/plan_furniture.star", "PIECES", "SETS", "SET_FOR_KIND", "SOURCES", "TALL",
         "set_items", "piece_shapes", "catalog_probe")

What a room must hold, written down once so the room stage (`@cadpole/plan_furnish.star`), the fit tables
(`@cadpole/plan_fit_tables.star`) and the audit all pack the same things. Nothing here solves anything:
it is plain dicts and tuples, built by a few pure helpers when the module loads.

**Units.** This catalog is in INCHES, the unit every source states its clearances in (NKBA's 42-in
aisle, CPC 402.5's 24 in). `@cadpole/plan_furnish.star` takes rooms in feet and returns feet; it is
the only place inches turn into feet.

## A piece

`PIECES[name]` is one kind of furniture or fixture:

* `shapes`: one or more rigid drawings of it (mirror images, a bed centred or tucked into a corner).
  A shape is `{"w", "d", "rects", "points", "corner"}` in the piece's own frame: `u` along the wall
  it backs onto (0 at one end), `v` out from that wall into the room. `w` is its length along the
  wall, `d` its solid depth.
  * `rects`: `(u0, v0, u1, v1, kind, tag)`. `kind` is `"solid"` (the piece itself: nothing else's
    solid or clearance may enter it) or `"clear"` (its use space: other clearances, door swings and
    walking paths may share it, solids may not). A clear rect may also be `"slide"`: a use space
    `u1 - u0` long that may sit anywhere along the piece's length (a tub's 30-in step-in space).
  * `points`: the use points a person has to reach, on the use side (the re-walk checks each one).
  * `corner`: `None`, or `"lo"` / `"hi"` when the shape only works pushed into the wall's low or high
    corner (a bed with its long side on a wall).
* `wall`: True when it backs onto a wall (one binary per wall and shape), False when it stands free
  (rotated 0 or 90 degrees: a dining table, an island).
* `h`: its height in inches. Under a window only pieces no taller than the sill may stand; `TALL`
  (48 in) is the sill the fit tables assume for their free window zone, so "tall" means taller than
  that (closets, shelves, a fridge).
* `wet`: a plumbing fixture (it drains): it goes on a stack wall when the room has one.
* `fixture`: reported as a plan `fixtures` record rather than `furniture`.
* `source`: where its size and clearances come from (keys of `SOURCES`).

## A set

`SETS[name]` is what one room holds: `{"label", "kind", "items", "window", "min_d", "extra", "source"}`.
`items` lists instances `{"name", "piece", "walls", "optional", "bonus", "h"}`: `walls` is
`"any"` (any wall not open to another room), a tuple of walls, or `None` for a free-standing
piece; an optional item earns `bonus` inches of depth in the room stage's objective when placed;
`h` overrides the piece's height. `window` is the length (in) of the escape-window zone a fit
table assumes on the window wall; `min_d` the least depth (in) worth trying; `extra` names extra
rule sets (`"triangle"`: NKBA's work triangle).

The fit tables' room types map to sets by `FIT_TABLE_KIND`: `full` is the child's room, `queen` the
primary bedroom, `tub` and `shower` the two baths, and so on.

## Code readings (unverified)

Every clearance below is a reading of the cited section, not checked with the building
department. CPC 402.5's 24 in in front of a water closet or lavatory is
an open question.

Probe for the purity test: `catalog_probe()`.
"""

# ---------------------------------------------------------------------------------------------- sources

SOURCES = {
    "CPC 402.5": "California Plumbing Code 2022 (UPC 2021 as adopted), 402.5 Setting: a water closet no closer than 15 in from its centre to a side wall or obstruction, 30 in centre to centre between fixtures; not less than 24 in of clear space in front of a water closet or lavatory. CRC R307.1 sends fixture spacing to the CPC. Reading unverified.",
    "CPC 407": "California Plumbing Code 2022, 407 Lavatories (the fixture; its clear space is 402.5's 24 in). Reading unverified.",
    "CPC 408": "California Plumbing Code 2022, 408 Showers: 408.6, a shower compartment of at least 1,024 sq in finished interior that can hold a 30-in circle, kept to 70 in above the drain (as the City of Albany's curbless-shower handout restates it). Reading unverified.",
    "CRC R307.1": "California Residential Code 2022, R307.1 Space Required: fixtures spaced per the California Plumbing Code (California replaces the IRC's Figure R307.1).",
    "IRC R307.1": "International Residential Code 2021, Figure R307.1 / P2705.1: 15 in from a water closet's centre to a wall, 21 in clear in front of a water closet or lavatory, 24 in in front of a shower's entrance. The model code; California does not adopt the figure. Reading unverified.",
    "NKBA KPG": "NKBA Kitchen Planning Guidelines (as read here): KPG 3 work triangle, each leg 4 to 9 ft, total at most 26 ft; KPG 6 work aisle 42 in for one cook; KPG 11 sink landing 24 in one side, 18 in the other; KPG 13 dishwasher within 36 in of the sink; KPG 16 refrigerator landing 15 in on the handle side; KPG 17 cooking-surface landing 12 in one side, 15 in the other; KPG 24 two landings that meet: the longer plus 12 in. Reading unverified.",
    "NKBA BPG": "NKBA Bathroom Planning Guidelines: 30 in of clear floor in front of a tub or shower (a recommendation; the codes ask 24 in at a shower entrance). Reading unverified.",
    "typical": "Typical furniture sets, sizes and clearances for a family house (beds 24 to 36 in round, a dining table for 8 with 36 in round, NKBA kitchen runs, a 10 x 20-ft car).",
    "small bath": "A small-bath model: a WC in a 30-in space, a 30 x 21 vanity, a 60 x 30 tub whose 30-in step-in space slides along it, 36 x 36 and 32 x 32 showers.",
    "Merrell 2011": "Merrell, Schkufza, Li, Agrawala, Koltun, Interactive Furniture Layout Using Interior Design Guidelines (SIGGRAPH 2011), Table 1 after Panero and Repetto: bedside 36 in, a seat 30 in in front, cabinets 24 in in front.",
    "OSU EC 691": "Oregon State University Extension Circular 691 (closets): a reach-in 24 to 30 in deep, at least 4 ft of rod a person.",
}

TALL = 48.0  # in: the sill a fit table's free window zone assumes; pieces taller than this keep out of it

# ---------------------------------------------------------------------------------------------- helpers

def _r(u0, v0, u1, v1, kind, tag):
    return (float(u0), float(v0), float(u1), float(v1), kind, tag)

def _shape(w, d, rects, points = None, corner = None):
    return {"w": float(w), "d": float(d), "rects": rects, "points": points if points != None else {}, "corner": corner}

def _flip(sh):
    """The mirror image of a shape along its wall (left <-> right); the rects and points keep their order."""
    w = sh["w"]
    rects = [(w - u1, v0, w - u0, v1, k, t) for (u0, v0, u1, v1, k, t) in sh["rects"]]
    pts = {}
    for k, (u, v) in sh["points"].items():
        pts[k] = (w - u, v)
    corner = {"lo": "hi", "hi": "lo"}.get(sh["corner"]) if sh["corner"] != None else None
    return _shape(w, sh["d"], rects, pts, corner)

def _wall_piece(w, d, front, tag, pt = None):
    """A piece against a wall: its solid w x d, then `front` inches of use space in front of it, the use point
    in that space (18 in out, or less when the space is shallower)."""
    rects = [_r(0, 0, w, d, "solid", tag)]
    if front > 0:
        rects.append(_r(0, d, w, d + front, "clear", tag + " front"))
    pts = {tag: (w / 2.0, d + min(front, 18.0))} if pt == None else pt
    return _shape(w, d, rects, pts)

def _piece(label, shapes, h, wall = True, wet = False, fixture = False, source = ()):
    return {"label": label, "shapes": shapes, "h": float(h), "wall": wall, "wet": wet, "fixture": fixture, "source": list(source)}

# ---------------------------------------------------------------------------------------------- beds

def _queen_bed(side = 30.0, foot = 36.0, bw = 60.0):
    """A queen 60 x 80 (a king: bw = 76) with a 4-in headboard, two 24 x 18 nightstands, `side` in clear at each side,
    `foot` at the foot."""
    w = 2 * side + bw
    rects = [_r(side, 0, side + bw, 84, "solid", "bed"),
             _r(side - 24, 0, side, 18, "solid", "nightstand"), _r(side + bw, 0, side + bw + 24, 18, "solid", "nightstand"),
             _r(0, 18, side, 84, "clear", "bedside"), _r(side + bw, 18, w, 84, "clear", "bedside"),
             _r(0, 84, w, 84 + foot, "clear", "foot")]
    return [_shape(w, 84.0, rects, {"bedL": (side / 2.0, 51.0), "bedR": (w - side / 2.0, 51.0)})]

def _kid_bed(bw, bl, ns = 22.0, side = 24.0, foot = 24.0, head = 3.0):
    """A child's bed bw x bl (plus a `head`-in headboard): centred with `side` in each side and `foot` at the foot, or
    tucked into a corner with its long side on a wall (`side` on the open side and at the foot). One ns x 18 nightstand.
    Returns [centred, corner (lo), corner (hi)]. The full bed is _kid_bed(54, 75)."""
    L = bl + head
    w = 2 * side + bw
    c = _shape(w, L, [_r(side, 0, side + bw, L, "solid", "bed"), _r(0, 0, ns, 18, "solid", "nightstand"),
                      _r(0, 18, side, L, "clear", "bedside"), _r(side + bw, 0, w, L, "clear", "bedside"),
                      _r(0, L, w, L + foot, "clear", "foot")], {"bed": (w - side / 2.0, 50.0)})
    k = _shape(bw + ns + 24.0, L, [_r(0, 0, bw, L, "solid", "bed"), _r(bw, 0, bw + ns, 18, "solid", "nightstand"),
                                    _r(bw, 18, bw + side, L, "clear", "bedside"), _r(0, L, bw + side, L + foot, "clear", "foot")],
               {"bed": (bw + side / 2.0, 50.0)}, corner = "lo")
    return [c, k, _flip(k)]

def _bunk(bw = 41.0, bl = 80.0, side = 30.0, foot = 24.0):
    """A twin-over-twin bunk, its ladder at the foot: centred (no nightstand; `side` in at each side, `foot` at the foot)
    or along a wall in a corner. 30 in beside it to make the lower bed."""
    w = 2 * side + bw
    c = _shape(w, bl, [_r(side, 0, side + bw, bl, "solid", "bunk"), _r(0, 0, side, bl, "clear", "bedside"),
                       _r(side + bw, 0, w, bl, "clear", "bedside"), _r(0, bl, w, bl + foot, "clear", "foot")],
               {"bed": (w - side / 2.0, 40.0)})
    k = _shape(bw + side, bl, [_r(0, 0, bw, bl, "solid", "bunk"), _r(bw, 0, bw + side, bl, "clear", "bedside"),
                               _r(0, bl, bw + side, bl + foot, "clear", "foot")], {"bed": (bw + side / 2.0, 40.0)}, corner = "lo")
    return [c, k, _flip(k)]

# ---------------------------------------------------------------------------------------------- bath fixtures

def _wc(front = 24.0):
    """A water closet in its 30-in space (15 in from its centre to a side wall or fixture), 28 in deep, `front` in clear in front."""
    return [_shape(30.0, 28.0, [_r(0, 0, 30, 28, "solid", "wc"), _r(0, 28, 30, 28 + front, "clear", "wc front")], {"wc": (15.0, 28.0 + 0.5 * front + 4.0)})]

def _lav(w = 36.0, d = 21.0, front = 24.0):
    return [_shape(w, d, [_r(0, 0, w, d, "solid", "lav"), _r(0, d, w, d + front, "clear", "lav front")], {"lav": (w / 2.0, d + 0.5 * front)})]

def _double_vanity(front = 24.0):
    return [_shape(66.0, 22.0, [_r(0, 0, 66, 22, "solid", "vanity"), _r(0, 22, 66, 22 + front, "clear", "vanity front")],
                   {"sink1": (16.5, 22.0 + 0.5 * front), "sink2": (49.5, 22.0 + 0.5 * front)})]

def _tub(front = 30.0):
    """An alcove tub 60 x 30, `front` in clear along its whole open side."""
    return [_shape(60.0, 30.0, [_r(0, 0, 60, 30, "solid", "tub"), _r(0, 30, 60, 30 + front, "clear", "tub front")], {"tub": (30.0, 30.0 + 0.5 * front)})]

def _tub_slide(front = 24.0, step = 30.0):
    """The small-bath tub: 60 x 30 with a `step`-in-wide, `front`-in-deep step-in space that may sit anywhere along it."""
    return [_shape(60.0, 30.0, [_r(0, 0, 60, 30, "solid", "tub"), _r(0, 30, step, 30 + front, "slide", "tub front")], {"tub": (0.5 * step, 30.0 + 0.5 * front)})]

def _shower(w = 48.0, d = 36.0, front = 30.0):
    """A w x d shower entered from its long side, `front` in clear there. 48 x 36 is 1,728 sq in and holds a 30-in circle."""
    return [_shape(w, d, [_r(0, 0, w, d, "solid", "shower"), _r(0, d, w, d + front, "clear", "shower front")], {"shower": (w / 2.0, d + 0.5 * front)})]

def _shower_slide(w = 60.0, d = 36.0, front = 24.0, step = 30.0):
    """A w x d shower whose step-in space -- `step` wide, `front` deep (the IRC's 24 in at a shower's entrance) -- may sit
    anywhere along its open side, where its door is."""
    return [_shape(w, d, [_r(0, 0, w, d, "solid", "shower"), _r(0, d, step, d + front, "slide", "shower front")], {"shower": (0.5 * step, d + 0.5 * front)})]

def _small_fixture(a, b, front, cw, tag):
    """The small-bath fixtures: an a x b solid with a cw-wide, `front`-deep clear space centred on its front."""
    off = (a - cw) / 2.0
    return [_shape(a, b, [_r(0, 0, a, b, "solid", tag), _r(off, b, off + cw, b + front, "clear", tag + " front")], {"use": (a / 2.0, b + 0.5 * front)})]

# ---------------------------------------------------------------------------------------------- day rooms, kitchens, service

def _media_group():
    """The living room's conversation group backed onto its focal wall: a 60 x 18 console, two 32 x 34 armchairs facing each
    other across a 48 x 24 coffee table 18 in from the seats, the 84 x 38 sofa facing the focal wall about 10 ft from it, 30 in
    of walkway behind the sofa and 24 in beside the chairs."""
    rects = [_r(50, 0, 110, 18, "solid", "console"),
             _r(14, 58, 48, 90, "solid", "chair"), _r(112, 58, 146, 90, "solid", "chair"),
             _r(56, 62, 104, 86, "solid", "table"),
             _r(38, 104, 122, 142, "solid", "sofa"),
             _r(0, 18, 160, 58, "clear", "view"), _r(0, 142, 160, 172, "clear", "walk"),
             _r(0, 58, 14, 142, "clear", "side"), _r(146, 58, 160, 142, "clear", "side")]
    return [_shape(160.0, 142.0, rects, {"sofa": (80.0, 123.0), "chair": (31.0, 74.0)})]

def _dining8():
    """A 42 x 96 table for 8 with 36 in all round (chairs pull out; a person edges past)."""
    return [_shape(168.0, 114.0, [_r(36, 36, 132, 78, "solid", "table"), _r(0, 0, 168, 114, "clear", "36 in round")], {"table": (84.0, 57.0)})]

def _dining4():
    """A 36 x 48 table for 4 with 36 in all round."""
    return [_shape(108.0, 120.0, [_r(36, 36, 72, 84, "solid", "table"), _r(0, 0, 108, 120, "clear", "36 in round")], {"table": (54.0, 60.0)})]

def _sleeper_sofa():
    """An 84 x 38 sleeper sofa; opened, the bed reaches 90 in from the wall, and a guest needs 24 in beyond its foot."""
    return [_shape(84.0, 38.0, [_r(0, 0, 84, 38, "solid", "sofa"), _r(0, 38, 84, 114, "clear", "open bed + 24")], {"sofa": (42.0, 56.0)})]

def _sink_run():
    """24-in landing | 36-in sink base | 24-in dishwasher (the counter over it the 18-in landing), 84 in, 25 deep, 42 in in front."""
    s = _shape(84.0, 25.0, [_r(0, 0, 84, 25, "solid", "sink run"), _r(0, 25, 84, 67, "clear", "aisle")],
               {"sink": (42.0, 25.0), "coffee": (12.0, 25.0), "trash": (72.0, 25.0)})
    return [s, _flip(s)]

def _range_run():
    """15-in landing | 30-in range | 15-in landing, 60 in."""
    return [_shape(60.0, 25.0, [_r(0, 0, 60, 25, "solid", "range run"), _r(0, 25, 60, 67, "clear", "aisle")], {"range": (30.0, 25.0)})]

def _fridge_run():
    """A 36 x 30 counter-depth fridge with 15 in of landing on its handle side; two hands."""
    a = _shape(51.0, 30.0, [_r(0, 0, 36, 30, "solid", "fridge"), _r(36, 0, 51, 25, "solid", "landing"),
                            _r(0, 30, 51, 72, "clear", "aisle")], {"fridge": (18.0, 30.0)})
    return [a, _flip(a)]

def _island():
    """A 36 x 84 island with a 42-in aisle all round."""
    return [_shape(168.0, 120.0, [_r(42, 42, 126, 78, "solid", "island"), _r(0, 0, 168, 120, "clear", "42 in round")], {"island": (84.0, 90.0)})]

def _washer_dryer():
    """Front-loading washer and dryer side by side (2 x 27 in, 32 deep with the vent), a folding counter over them, 42 in in front."""
    return [_shape(54.0, 32.0, [_r(0, 0, 54, 32, "solid", "w/d"), _r(0, 32, 54, 74, "clear", "w/d front")], {"washer": (27.0, 50.0)})]

def _car_free():
    """One car anywhere along the garage-door wall: the 76 x 190 body centred across a 10 x 20-ft clear floor, 18 in in from
    the door (a garage whose car sits on its door's axis, not in a corner)."""
    return [_shape(120.0, 240.0, [_r(22, 18, 98, 208, "solid", "car"), _r(0, 0, 120, 240, "clear", "10 x 20 clear")], {"car": (11.0, 120.0)})]

def _car(w_clear = 120.0, depth = 240.0):
    """One car: a 10 x 20-ft clear floor (a person walks it) with the body, 76 x 190, 36 in off the left wall for the driver's
    door and 18 in in from the garage door; tucked into the left corner of the garage-door wall."""
    return [_shape(w_clear, depth, [_r(36, 18, 112, 208, "solid", "car"), _r(0, 0, w_clear, depth, "clear", "10 x 20 clear")], {"car": (24.0, 120.0)}, corner = "lo")]

# ---------------------------------------------------------------------------------------------- the pieces

_TYP = ("typical",)

PIECES = {
    # beds (the bed, its nightstands and its clear sides travel as one rigid piece)
    "bed_queen": _piece("queen bed, 2 nightstands, 30 in at the sides, 36 at the foot", _queen_bed(), 24, source = _TYP + ("Merrell 2011",)),
    "bed_king": _piece("king bed 76 x 80, 2 nightstands, 30 in at the sides, 36 at the foot", _queen_bed(30.0, 36.0, 76.0), 24, source = _TYP + ("Merrell 2011",)),
    "bed_queen_guest": _piece("queen bed, 2 nightstands, 24 in round", _queen_bed(24.0, 24.0), 24, source = _TYP),
    "bed_full": _piece("full bed 54 x 75, a nightstand, 24 in round", _kid_bed(54.0, 75.0), 24, source = _TYP),
    "bed_twin": _piece("twin bed 39 x 75, a nightstand, 24 in round", _kid_bed(39.0, 75.0), 24, source = _TYP),
    "bunk_twin": _piece("twin-over-twin bunk 41 x 80, 30 in beside it", _bunk(), 66, source = ("Merrell 2011",)),
    "desk": _piece("desk 48 x 24, 36 in for the chair", [_wall_piece(48.0, 24.0, 36.0, "desk")], 30, source = _TYP),
    "desk_60": _piece("desk 60 x 30, 36 in for the chair", [_wall_piece(60.0, 30.0, 36.0, "desk")], 30, source = _TYP),
    "dresser": _piece("dresser 36 x 18, 30 in in front", [_wall_piece(36.0, 18.0, 30.0, "dresser")], 34, source = _TYP),
    "dresser_60": _piece("dresser 60 x 20, 36 in in front", [_wall_piece(60.0, 20.0, 36.0, "dresser")], 34, source = _TYP),
    "reach_in": _piece("reach-in closet, 5 ft of rod, 24 deep, 30 in in front", [_wall_piece(60.0, 24.0, 30.0, "closet")], 96, source = _TYP + ("OSU EC 691",)),
    "sleeper_sofa": _piece("sleeper sofa 84 x 38, open 90 in, 24 in past its foot", _sleeper_sofa(), 34, source = _TYP),
    "bookshelf": _piece("bookshelf 36 x 14, 30 in in front", [_wall_piece(36.0, 14.0, 30.0, "books")], 84, source = _TYP),
    "credenza": _piece("credenza 60 x 20, 30 in in front", [_wall_piece(60.0, 20.0, 30.0, "credenza")], 30),
    # baths (CPC 402.5's 24 in in front of the WC and lavatory, NKBA's 30 in at the tub and shower)
    "wc": _piece("water closet in a 30-in space, 24 in clear in front", _wc(24.0), 30, wet = True, fixture = True, source = ("CPC 402.5", "CRC R307.1")),
    "wc_irc": _piece("water closet in a 30-in space, 21 in clear in front", _wc(21.0), 30, wet = True, fixture = True, source = ("IRC R307.1",)),
    "lav": _piece("lavatory vanity 36 x 21, 24 in clear in front", _lav(36.0, 21.0, 24.0), 34, wet = True, fixture = True, source = ("CPC 402.5", "CPC 407")),
    "lav_30": _piece("lavatory vanity 30 x 21, 24 in clear in front", _lav(30.0, 21.0, 24.0), 34, wet = True, fixture = True, source = ("CPC 402.5", "CPC 407")),
    "lav_small": _piece("lavatory 24 x 18, 24 in clear in front", _lav(24.0, 18.0, 24.0), 34, wet = True, fixture = True, source = ("CPC 402.5", "CPC 407")),
    "lav_irc": _piece("lavatory vanity 30 x 21, 21 in clear in front", _lav(30.0, 21.0, 21.0), 34, wet = True, fixture = True, source = ("IRC R307.1",)),
    "vanity_double": _piece("double vanity 66 x 22, 24 in clear in front", _double_vanity(24.0), 34, wet = True, fixture = True, source = ("CPC 402.5", "CPC 407")),
    "tub": _piece("alcove tub 60 x 30, 30 in clear along it", _tub(30.0), 20, wet = True, fixture = True, source = ("NKBA BPG",)),
    "tub_72": _piece("tub 72 x 36 against a wall, a 30-in-wide step-in 24 in deep anywhere along it", [_shape(72.0, 36.0, [_r(0, 0, 72, 36, "solid", "tub"), _r(0, 36, 30, 60, "slide", "tub front")], {"tub": (15.0, 48.0)})], 22, wet = True, fixture = True, source = ("NKBA BPG", "IRC R307.1")),
    "shower": _piece("shower 48 x 36, 30 in clear at its entrance", _shower(48.0, 36.0, 30.0), 80, wet = True, fixture = True, source = ("CPC 408", "NKBA BPG")),
    "shower_60": _piece("shower 60 x 36, 30 in clear at its entrance", _shower(60.0, 36.0, 30.0), 80, wet = True, fixture = True, source = ("CPC 408", "NKBA BPG")),
    "shower_60s": _piece("shower 60 x 36, a 30-in-wide, 24-in-deep step-in anywhere along it", _shower_slide(60.0, 36.0, 24.0, 30.0), 80, wet = True, fixture = True, source = ("CPC 408", "IRC R307.1")),
    "shower_36": _piece("shower 36 x 36, 24 in clear at its entrance", _shower(36.0, 36.0, 24.0), 80, wet = True, fixture = True, source = ("CPC 408", "IRC R307.1")),
    # the small bath: 30-wide clear spaces centred on the WC and the vanity, a sliding 24 x 30 step-in at the tub
    "small_wc": _piece("water closet 30 x 28, 30-in-wide clear space in front (small bath)", _small_fixture(30.0, 28.0, 24.0, 30.0, "wc"), 30, wet = True, fixture = True, source = ("small bath", "CPC 402.5")),
    "small_wc_irc": _piece("water closet 30 x 28, 21 in clear (small bath, IRC)", _small_fixture(30.0, 28.0, 21.0, 30.0, "wc"), 30, wet = True, fixture = True, source = ("small bath", "IRC R307.1")),
    "small_lav": _piece("vanity 30 x 21, 30-in-wide clear space in front (small bath)", _small_fixture(30.0, 21.0, 24.0, 30.0, "lav"), 34, wet = True, fixture = True, source = ("small bath", "CPC 402.5")),
    "small_lav_irc": _piece("vanity 30 x 21, 21 in clear (small bath, IRC)", _small_fixture(30.0, 21.0, 21.0, 30.0, "lav"), 34, wet = True, fixture = True, source = ("small bath", "IRC R307.1")),
    "small_tub": _piece("tub 60 x 30, a 30-wide 24-deep step-in space anywhere along it (small bath)", _tub_slide(24.0, 30.0), 20, wet = True, fixture = True, source = ("small bath", "IRC R307.1")),
    # kitchens (NKBA): 25 deep, a 42-in work aisle in front
    "sink_run": _piece("sink run: 24-in landing, 36-in sink base, 24-in dishwasher", _sink_run(), 36, wet = True, source = ("NKBA KPG",) + _TYP),
    "range_run": _piece("range run: 15-in landings each side of a 30-in range", _range_run(), 36, source = ("NKBA KPG",) + _TYP),
    "fridge_run": _piece("36 x 30 fridge with a 15-in landing on its handle side", _fridge_run(), 70, source = ("NKBA KPG",) + _TYP),
    "island": _piece("36 x 84 island, 42 in round", _island(), 36, wall = False, source = ("NKBA KPG",) + _TYP),
    # day rooms
    "media_group": _piece("sofa, two chairs, coffee table and console, about 10 ft from the focal wall", _media_group(), 34, source = _TYP),
    "dining8": _piece("table for 8, 42 x 96, 36 in round", _dining8(), 30, wall = False, source = _TYP + ("NKBA KPG",)),
    "dining4": _piece("table for 4, 36 x 48, 36 in round", _dining4(), 30, wall = False, source = _TYP),
    "sideboard": _piece("sideboard 60 x 18, 30 in in front", [_wall_piece(60.0, 18.0, 30.0, "sideboard")], 36, source = _TYP),
    "bench": _piece("bench 48 x 18, 24 in in front", [_wall_piece(48.0, 18.0, 24.0, "bench")], 18, source = _TYP),
    "coat_closet": _piece("coat closet 48 x 24, 30 in in front", [_wall_piece(48.0, 24.0, 30.0, "coats")], 96, source = _TYP),
    # service
    "washer_dryer": _piece("washer and dryer side by side, 54 x 32, 42 in in front", _washer_dryer(), 38, wet = True, source = _TYP),
    "linen": _piece("linen shelves 36 x 16, 30 in in front", [_wall_piece(36.0, 16.0, 30.0, "linen")], 84, source = _TYP),
    "pantry_shelf": _piece("pantry shelves 48 x 16, 36 in in front", [_wall_piece(48.0, 16.0, 36.0, "shelves")], 84, source = _TYP),
    "rod_6ft": _piece("6 ft of hanging rod, 24 deep, 36 in in front", [_wall_piece(72.0, 24.0, 36.0, "rod 6 ft")], 84, source = _TYP + ("OSU EC 691",)),
    "bins": _piece("storage bins 72 x 30, 24 in in front", [_wall_piece(72.0, 30.0, 24.0, "bins")], 40, source = _TYP),
    "garage_shelf": _piece("garage shelving 96 x 24, 30 in in front", [_wall_piece(96.0, 24.0, 30.0, "shelf")], 84, source = _TYP),
    "car": _piece("one car: a 10 x 20-ft clear floor, the body 76 x 190", _car(), 60, source = _TYP),
    "car_free": _piece("one car anywhere along the door wall: the body 76 x 190 centred in a 10 x 20-ft clear floor", _car_free(), 60, source = _TYP),
}

def piece_shapes(name):
    """The shapes of PIECES[name] (a list; the garage's car is built for the room's width by plan_furnish)."""
    return PIECES[name]["shapes"]

# ---------------------------------------------------------------------------------------------- the sets

def _it(name, piece, walls = "any", optional = False, bonus = 0.0, h = None):
    return {"name": name, "piece": piece, "walls": walls, "optional": optional, "bonus": float(bonus), "h": h}

def _set(label, kind, items, window = None, min_d = 60.0, extra = (), source = (), dl = None):
    return {"label": label, "kind": kind, "items": items, "window": window, "min_d": float(min_d), "extra": list(extra), "source": list(source), "fit_kind": dl}

SETS = {
    # bedrooms
    "full": _set("full bed, desk, dresser, 5 ft of closet", "bed",
                 [_it("bed", "bed_full"), _it("desk", "desk"), _it("dresser", "dresser"), _it("closet", "reach_in")],
                 window = 36.0, min_d = 84.0, source = _TYP, dl = "kid"),
    "twin": _set("twin bed, desk, dresser, 5 ft of closet", "bed",
                 [_it("bed", "bed_twin"), _it("desk", "desk"), _it("dresser", "dresser"), _it("closet", "reach_in")],
                 window = 36.0, min_d = 84.0, source = _TYP),
    "bunk": _set("twin bunk, desk, dresser, 5 ft of closet", "bed",
                 [_it("bed", "bunk_twin"), _it("desk", "desk"), _it("dresser", "dresser"), _it("closet", "reach_in")],
                 window = 36.0, min_d = 84.0, source = _TYP + ("Merrell 2011",)),
    "queen": _set("queen bed with 2 nightstands, 60-in dresser", "bed",
                  [_it("bed", "bed_queen"), _it("dresser", "dresser_60")],
                  window = 36.0, min_d = 120.0, source = _TYP, dl = "primary"),
    "king": _set("king bed with 2 nightstands, 60-in dresser", "bed",
                 [_it("bed", "bed_king"), _it("dresser", "dresser_60")],
                 window = 36.0, min_d = 120.0, source = _TYP),
    "queen_desk": _set("queen bed with 2 nightstands, 60-in dresser, desk", "bed",
                       [_it("bed", "bed_queen"), _it("dresser", "dresser_60"), _it("desk", "desk")],
                       window = 36.0, min_d = 120.0, source = _TYP),
    "guest": _set("queen bed with 2 nightstands, 24 in round; dresser", "guest",
                  [_it("bed", "bed_queen_guest"), _it("dresser", "dresser")],
                  window = 36.0, min_d = 108.0, source = _TYP),
    # baths (California: CPC 402.5's 24 in in front of the WC and lavatory)
    "tub": _set("tub, WC, 36-in vanity", "bath", [_it("tub", "tub"), _it("wc", "wc"), _it("lav", "lav")],
                min_d = 60.0, source = ("CPC 402.5", "CPC 407", "NKBA BPG"), dl = "bath2"),
    "shower": _set("48 x 36 shower, WC, 30-in vanity", "bath", [_it("shower", "shower"), _it("wc", "wc"), _it("lav", "lav_30")],
                   min_d = 60.0, source = ("CPC 402.5", "CPC 407", "CPC 408"), dl = "bath1"),
    "shower_60": _set("60 x 36 shower, WC, 36-in vanity", "bath", [_it("shower", "shower_60"), _it("wc", "wc"), _it("lav", "lav")],
                      min_d = 60.0, source = ("CPC 402.5", "CPC 407", "CPC 408")),
    "shower_60s": _set("60 x 36 shower (its step-in where its door is), WC, 36-in vanity", "bath", [_it("shower", "shower_60s"), _it("wc", "wc"), _it("lav", "lav")],
                       min_d = 60.0, source = ("CPC 402.5", "CPC 407", "CPC 408", "IRC R307.1")),
    "pbath_tub": _set("double vanity, 60 x 36 shower, 72 x 36 tub, WC", "bath",
                      [_it("vanity", "vanity_double"), _it("shower", "shower_60s"), _it("tub", "tub_72"), _it("wc", "wc")],
                      min_d = 60.0, source = ("CPC 402.5", "CPC 407", "CPC 408", "NKBA BPG")),
    "pbath": _set("double vanity, 60 x 36 shower, WC", "bath", [_it("vanity", "vanity_double"), _it("shower", "shower_60"), _it("wc", "wc")],
                  min_d = 60.0, source = ("CPC 402.5", "CPC 407", "CPC 408"), dl = "pbath"),
    "powder": _set("WC and a small lavatory", "powder", [_it("wc", "wc"), _it("lav", "lav_small")],
                   min_d = 48.0, source = ("CPC 402.5", "CPC 407"), dl = "powder"),
    "tub_irc": _set("tub, WC, 30-in vanity at the IRC's 21 in", "bath", [_it("tub", "tub"), _it("wc", "wc_irc"), _it("lav", "lav_irc")],
                    min_d = 60.0, source = ("IRC R307.1", "NKBA BPG")),
    # the small-bath model: a tub whose step-in space slides, CPC's 24 in (or the IRC's 21 in)
    "small_tub": _set("tub, WC, 30-in vanity (the small-bath model, CPC 24 in)", "bath",
                   [_it("tub", "small_tub"), _it("wc", "small_wc"), _it("lav", "small_lav")], min_d = 60.0, source = ("small bath", "CPC 402.5")),
    "small_tub_irc": _set("tub, WC, 30-in vanity (the small-bath model, IRC 21 in)", "bath",
                       [_it("tub", "small_tub"), _it("wc", "small_wc_irc"), _it("lav", "small_lav_irc")], min_d = 60.0, source = ("small bath", "IRC R307.1")),
    # kitchens (NKBA)
    "kitchen_L": _set("sink run, range run, fridge (NKBA triangle)", "kitchen",
                      [_it("sinkrun", "sink_run"), _it("rangerun", "range_run"), _it("fridge", "fridge_run")],
                      window = 36.0, min_d = 96.0, extra = ("triangle",), source = ("NKBA KPG",) + _TYP, dl = "kitchen"),
    "kitchen_island": _set("sink run, range run, fridge and an island (NKBA triangle)", "kitchen",
                           [_it("sinkrun", "sink_run"), _it("rangerun", "range_run"), _it("fridge", "fridge_run"), _it("island", "island", walls = None)],
                           window = 36.0, min_d = 96.0, extra = ("triangle",), source = ("NKBA KPG",) + _TYP, dl = "kitchen"),
    "kitchen": _set("sink run, range run, fridge, an island if it fits (NKBA triangle)", "kitchen",
                    [_it("sinkrun", "sink_run"), _it("rangerun", "range_run"), _it("fridge", "fridge_run"),
                     _it("island", "island", walls = None, optional = True, bonus = 24.0)],
                    window = 36.0, min_d = 96.0, extra = ("triangle",), source = ("NKBA KPG",) + _TYP),
    # day rooms
    "living": _set("sofa, two chairs, coffee table, console", "living", [_it("group", "media_group")], window = 36.0, min_d = 120.0, source = _TYP, dl = "living"),
    "dining": _set("table for 8, a sideboard if it fits", "dining",
                   [_it("table", "dining8", walls = None), _it("sideboard", "sideboard", optional = True, bonus = 6.0)],
                   window = 36.0, min_d = 114.0, source = _TYP, dl = "dining"),
    "dining4": _set("table for 4", "dining", [_it("table", "dining4", walls = None)], window = 36.0, min_d = 96.0, source = _TYP),
    "den": _set("sleeper sofa, 60-in desk, bookshelf", "den",
                [_it("sofa", "sleeper_sofa"), _it("desk", "desk_60"), _it("books", "bookshelf")],
                window = 36.0, min_d = 96.0, source = _TYP, dl = "den"),
    "office": _set("60-in desk, credenza, bookshelf", "office",
                   [_it("desk", "desk_60"), _it("credenza", "credenza"), _it("books", "bookshelf")],
                   window = 36.0, min_d = 84.0, source = _TYP),
    "desk": _set("a 60-in desk and its chair", "office", [_it("desk", "desk_60")], window = 36.0, min_d = 60.0, source = _TYP),
    "entry": _set("coat closet and a bench", "entry", [_it("coats", "coat_closet"), _it("bench", "bench")], min_d = 72.0, source = _TYP, dl = "entry"),
    # service
    "laundry": _set("washer and dryer, linen shelves", "laundry", [_it("w/d", "washer_dryer"), _it("linen", "linen")], min_d = 60.0, source = _TYP, dl = "laundry"),
    "pantry": _set("12 ft of pantry shelving", "pantry", [_it("shelf%d" % k, "pantry_shelf") for k in range(3)], min_d = 48.0, source = _TYP, dl = "pantry"),
    "walkin": _set("12 ft of rod for two (a walk-in)", "closet", [_it("rod%d" % k, "rod_6ft") for k in range(2)], min_d = 60.0, source = _TYP + ("OSU EC 691",), dl = "closet"),
    # garages: the car on the garage-door wall (S in the room's frame), tucked into its left corner
    "car": _set("one car, 10 x 20 ft clear, in the corner by the door wall", "garage", [_it("car", "car", walls = ("S",))], min_d = 240.0, source = _TYP, dl = "garage"),
    "car_free": _set("one car, 10 x 20 ft clear, anywhere along the garage-door wall", "garage", [_it("car", "car_free", walls = ("S",))], min_d = 240.0, source = _TYP),
    "car_storage": _set("one car, storage bins, shelving", "garage",
                        [_it("car", "car", walls = ("S",)), _it("bins", "bins"), _it("shelf", "garage_shelf")], min_d = 240.0, source = _TYP, dl = "garage"),
}

# the set a room holds when the plan names none (`room["furnish"]`), by the plan format's room kinds
SET_FOR_KIND = {
    "bed": "full", "guest": "guest", "den": "den", "study": "office", "office": "office", "living": "living",
    "family": "living", "media": "living", "dining": "dining", "kitchen": "kitchen", "bath": "shower", "powder": "powder",
    "laundry": "laundry", "pantry": "pantry", "closet": "walkin", "garage": "car_free", "entry": "entry", "foyer": "entry", "mud": "entry",
}

# the fit tables' room types, by the set of each
FIT_TABLE_KIND = {"kid": "full", "primary": "queen", "bath2": "tub", "bath1": "shower", "pbath": "pbath", "powder": "powder",
                    "laundry": "laundry", "pantry": "pantry", "closet": "walkin", "den": "den", "living": "living", "dining": "dining",
                    "kitchen": "kitchen_L", "entry": "entry", "garage": "car"}

def set_items(name):
    """The items of SETS[name], each with its piece's shapes, height and flags resolved:
    [{"name", "piece", "shapes", "walls", "optional", "bonus", "h", "wet", "fixture"}]."""
    if name not in SETS:
        fail("plan_furniture: no set named %r (have: %s)" % (name, ", ".join(sorted(SETS.keys()))))
    out = []
    for it in SETS[name]["items"]:
        pc = PIECES[it["piece"]]
        walls = it["walls"]
        if not pc["wall"]:
            walls = None
        out.append({"name": it["name"], "piece": it["piece"], "shapes": pc["shapes"], "walls": walls, "optional": it["optional"],
                    "bonus": it["bonus"], "h": it["h"] if it["h"] != None else pc["h"], "wet": pc["wet"], "fixture": pc["fixture"]})
    return out

def catalog_probe():
    """The purity probe: a pure function of the catalog (the number of pieces and sets, and the full bed's shapes)."""
    return (len(PIECES), len(SETS), len(PIECES["bed_full"]["shapes"]))
