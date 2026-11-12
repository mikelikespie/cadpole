# Parts and assemblies

Mechanical work in Cadpole: a part, a mechanism, an enclosure, a whole
assembly, to print, machine or draw. This document is what that work adds
to the shared core. The language, sketches, solids, booleans, measurement,
exports, drawings and tests are in [`starlark-api.md`](starlark-api.md), and
a part script uses little else. For buildings see
[`buildings.md`](buildings.md); a part script never needs it.

A section reference is written §n: plain numbers are sections of
`starlark-api.md`, §P numbers are sections of this document and §B numbers
are in `buildings.md`.

| Section | What is in it |
| --- | --- |
| [§P1](#p1-what-it-is-for) What it is for | the work and the examples |
| [§P2](#p2-a-first-part) A first part | a script to run and change |
| [§P3](#p3-commands-and-units) Commands and units | the CLI for parts; millimetres |
| [§P4](#p4-assemblies) Assemblies | `part`, `instance`, `assembly`, `frame`, `place`, `explode` |
| [§P5](#p5-gears-cadpolegearsstar) Gears | `@cadpole/gears.star`: spur, helical, herringbone, internal, rack, bevel, planetary sets |
| [§P6](#p6-springs-threads-and-fasteners-cadpolemechanicalstar) Springs, threads and fasteners | `@cadpole/mechanical.star`: `spring`, `thread`, `bolt`, `nut` |

## P1. What it is for

Single parts (`examples/bracket.star`, `examples/planter.star`,
`examples/knurled_knob.star`), parts built round other parts
(`examples/enclosure.star`), mechanisms that move
(`examples/gear_toy.star`, `examples/iris.star`, `examples/planetary.star`)
and assemblies with exploded and cutaway views (`examples/turbojet.star`,
`examples/radial_engine.star`). Threads and fasteners are in
`examples/thumbscrew.star` and `examples/spring_and_bolt.star`, gears in
`examples/gears.star`, printing in `examples/print_ready.star` and
`examples/split_for_bed.star`, a dimensioned sheet in
`examples/drawing_sheet.star`.

## P2. A first part

```python
w = param("width", 40, unit="mm", min=10, max=100)
base = extrude(sketch(XY).rect(w, 20), 10, name="base")
hole = extrude(sketch(XY.offset(-1)).circle(3), 12, name="hole")
part = subtract(base, hole, name="part")
export(part, "part.stl")
```

`examples/part.star` grows this plate into a mounting plate, with rounded
corners, an edge fillet, a centre bore and two counterbored screw holes whose
spacing follows the width, and adds its tests. Render it, change the width,
and open it in the web UI:

```
cadpole render examples/part.star -o part.png
cadpole render examples/part.star -p width=90 -o wide.png
cadpole serve examples/part.star
```

Each call is in `starlark-api.md`: `param` §4.1, `sketch` §4.3, `extrude`
§4.4, `subtract` §4.5, `export` §6.1; `fillet`, `cylinder` and `cone` are
in §4.4 too.

## P3. Commands and units

Lengths are millimetres and angles degrees, always (§1). `units(units.inch)`
(§4.1) changes how lengths are written back, never what a number is.

```
cadpole eval examples/bracket.star                       # the feature tree and diagnostics
cadpole render examples/planter.star -o planter.png      # a picture
cadpole sweep examples/planter.star --param top_radius=40,55,70 -o sheet.png   # a sheet of variants
cadpole animate examples/gear_toy.star -o toy.gif        # the script's animation(...) declarations
cadpole test examples/planter.star                       # the script's test(...) declarations
cadpole inspect examples/impeller.star                   # per body: counts, solid verdict, kernel decisions
cadpole check examples/print_ready.star                  # printability
cadpole export examples/bracket.star -o bracket.step     # STL, OBJ, 3MF, STEP, glTF by extension
cadpole export examples/drawing_sheet.star --run-exports --out-dir .   # dimensioned SVG, PDF and DXF drawings
cadpole serve examples/bracket.star                      # the web UI
```

`cadpole --help` lists every command. Printing and exports are §6.1,
drawings §6.3, tests §7.1.

---

## P4. Assemblies

A **part** is built once; an **instance** is a placed copy of it — a pose,
sharing the part's geometry, tessellation and face names, so ten instances
of a wheel cost one wheel; an **assembly** groups instances so a view can
explode them; a **frame** is a coordinate system read off a named face,
edge or vertex, and `place` maps one onto another. Nothing here solves
anything: an instance is where the script puts it. `explode` is a picture —
a per-instance offset applied when a scene is drawn, never geometry — so
`distance()`, `print_check` and every export see the assembled position.

```python
wheel_p = part(wheel, name="wheel_part")                   # wheel stops rendering on its own
shaft_p = part(rod, name="shaft_part")
wheel = instance(wheel_p, name="wheel")
shaft = instance(shaft_p, name="shaft", material="steel")
cap = instance(nut_p, name="cap")
place(shaft, shaft.face("rod/side[0.0]"), wheel.face("bore_cut/side[0.0]"), flush=True)   # axis onto axis
place(shaft, shaft.face("rod/start"), wheel.face("backplate/start"), flush=True, offset=shaft_len)
place(cap, cap.face("nut_hex/start"), wheel.face("hub/end"))                            # seat on the nose
pump = assembly([wheel, shaft, cap], name="pump")
lift = param("explode", 0.0, unit="mm", min=0, max=60)
explode(pump, lift, along=along.z)                          # cadpole render --param explode=20
angle = param("angle", 0.0, unit="deg")
pose(shaft, rotate=angle)                                   # kinematics-lite: cadpole sweep --param angle=0,90,180
c = distance(shaft, wheel)                                  # instances are bodies to every measurement
export(pump, "pump.3mf")                                    # one object per part, one build item per instance
```

| Call | Returns | Notes |
| --- | --- | --- |
| `part(body_or_bodies, *, name=None)` | Part | feature kind `part`; consumes its bodies; they keep their names. A body is in one part |
| `instance(part_or_body, *, at=(0,0,0), rotate=0, axis=(0,0,1), name=None, layer=None, material=None)` | Instance | feature kind `instance`; `rotate` is degrees about `axis` through the part's origin, before the move to `at`; a `Body` is wrapped in a part of its own; `material=` is a preset name. Bodies are `<instance>/<part body>` (`w3/wheel`) and their faces keep the part's names |
| `Instance.face(name)`, `.edge(name)` | Face / Edge | the part's name (`rod/side[0.0]`, or `side[0.0]` inside the part body's feature), where the instance puts it |
| `Instance.body(name=None)` | Body | the posed body: the only one, or the one whose part body is `name` |
| `Instance.frame()` | Frame | the instance's own origin and axes, now |
| `Instance.material(...)` | Instance | `Body.material`'s arguments, applied to every body of the instance |
| `Instance.bodies`, `.name`, `.id`, `.part` | | |
| `assembly(members, *, name=None)` | Assembly | feature kind `assembly`; members are instances and assemblies, each in one assembly |
| `Assembly.instances`, `.bodies`, `.name` | | every instance under it, depth first; their posed bodies |
| `place(instance, on, to, *, flush=False, offset=0, name=None)` | Instance | feature kind `place`. `on` (a Face, Edge, Frame, Plane or Instance on `instance`, read where it is now) lands on `to`: origins coincide, `x` axes align, `z` axes **oppose** — two faces look at each other — unless `flush=True`; `offset` moves along `to`'s `z`. A second `place` moves on from there |
| `pose(instance, *, translate=None, rotate=None, axis=(0,0,1), origin=None, name=None)` | Instance | feature kind `pose`: `rotate` degrees about `axis` through `origin` (the instance's own origin by default), then `translate`; both may be params |
| `explode(assembly, distance, *, along="z")` | None | feature kind `explode`, a view: members ranked by centre along `along` (`along.x`/`.y`/`.z`, or a direction `(x, y, z)`), lowest first, ties to the larger part which stays; rank *k* moves *k × distance*; equal centre and size share a rank. `along.radial` moves each member `distance` from the assembly's centre. A nested assembly moves as one member, then spreads its own |
| `explode(instance, (dx, dy, dz))` | None | an explicit view offset for one instance, added to its assemblies' |
| `frame(face_or_edge_or_instance_or_plane)` / `frame(origin=, z=, x=None)` | Frame | a face: centroid + normal, or the axis for a cylinder, cone or torus, the centre for a sphere; an edge: a circle's centre + axis, else midpoint + chord; `x` is the world axis least aligned with `z`, projected, unless given — a rule with no memory |
| `Frame.origin`, `.x`, `.y`, `.z`; `.offset(d)`, `.flip()`, `.turn(degrees)` | | |

`distance`, `clearance_map`, `print_check`, `assert_printable`, `export`
and `part` take an Instance or an Assembly wherever they take bodies;
`distance` between two many-body values is the closest pair over both.
`cadpole inspect` lists the instances. An instance's name is a feature id, so
it cannot repeat a body's (`sun_body` and `sun`); a script variable called
`explode` would shadow the call (§4.16), so bind the parameter to another
name. `examples/planetary.star` is the worked example: six instances, an
exploded view from `--param explode=`, and `sun_angle` posing the whole
train for a sweep through a tooth pitch.

There are no mates or joints and no constraint solver for assemblies.

---

## P5. Gears: `@cadpole/gears.star`

Involute gears as Sketch profiles and bodies, plus the arithmetic. All
lengths in mm, angles in degrees, tooth counts ints; keyword-only options
throughout. The file's docstring carries the formulas; this is the API.

Conventions: `module` is the **normal** module `m_n` (a helical gear's
transverse module is `m_n / cos(helix_angle)`, its pitch diameter
`m_n z / cos(helix_angle)`); `pressure_angle` the normal pressure angle (20);
`profile_shift` the coefficient `x` in modules; `backlash` the tooth
*thinning* at the pitch circle in mm (a pair thinned by `b` each has a
circumferential backlash `2 b` and a normal flank gap `b cos alpha`);
`clearance` (0.25) and `fillet` (0.38, ISO 53's rack tip radius) in modules;
`tolerance` (0.02 mm) the chord error the outline is thinned to. Tooth 0 of
an external gear is centred on +x; an internal gear's *space* 0 is on +x
(its teeth at `(k + ½) 360 / z`); a rack's tooth 0 is at x = 0 with its
pitch line on y = 0 and teeth pointing +y; a bevel gear's pitch-cone apex is
the origin, its axis +z. `hand` is `hand.right` / `hand.left` (§4.16): a
right-hand tooth turns counter-clockwise going up +z; a herringbone's hand
is its lower half's. Helical mates need the same module and helix angle and
opposite hands; a ring and its planet the same hand.

| Call | Returns | |
| --- | --- | --- |
| `involute_profile(module, teeth, *, pressure_angle=20, profile_shift=0, backlash=0, clearance=0.25, fillet=0.38, helix_angle=0, tolerance=0.02, internal=False, turn=0, plane=XY, name=None)` | Sketch | the full outline as one loop: true involute flanks, the trochoid the rack's rounded tip sweeps at the root; below `min_teeth` the real undercut (the trochoid crosses the involute and the flank is the trochoid below the crossing). Add a bore or keyway with the Sketch's loop methods and `extrude` it |
| `spur(module, teeth, width, *, bore=0, …profile options…, name=None)` | Body | one extrude, exact |
| `helical(module, teeth, width, helix_angle, *, hand="right", bore=0, …, name=None)` | Body | the transverse profile lofted smoothly (`loft(ruled=False)`) through copies turned by `dz tan(helix) / r_pitch`; one B-spline face per outline segment |
| `herringbone(module, teeth, width, helix_angle, *, hand="right", bore=0, …, name=None)` | Body | two helical halves of opposite hand, `glue`d on their shared mid-plane; the apex is sharp |
| `internal(module, teeth, width, rim, *, helix_angle=0, hand="right", herringbone=False, …, name=None)` | Body | a ring gear: the tooth space is the external tooth of the same base circle, addendum and dedendum swapped, a circular root fillet (`fillet * m` or the largest that fits), a small ring's addendum shortened to the base circle; the outer loop is a circle `rim` beyond the root |
| `rack(module, length, width, height, *, pressure_angle=20, backlash=0, clearance=0.25, fillet=0.38, tolerance=0.02, name=None)` | Body | straight flanks, arc root fillets (the rack is its own generator, so exact); no helical racks |
| `bevel(module, teeth, mate_teeth, *, face_width=None, shaft_angle=90, bore=0, …, name=None)` | Body | a straight bevel by Tredgold's back-cone approximation: the virtual spur gear of `z / cos(delta)` teeth mapped onto the transverse plane at the heel, a ruled loft to the scaled toe, so every flank line passes through the apex. Not the spherical involute (right at the heel, off by microns per mm toward the toe); a flat back, not a back cone |
| `layout(module, teeth, *, …profile options…, internal=False)` | dict | `teeth`, `segments` per tooth, `parts` (name → segment ranges within a tooth), `undercut`, `r_tip`, `r_root`, `r_pitch`, `r_base`; pure arithmetic |
| `tooth_faces(body, layout, tooth)` | dict of Selections | `root`, `fillet_right`, `flank_right`, `tip`, `flank_left`, `fillet_left` of tooth `tooth`, after any transform; on a herringbone both halves' faces. `flank_left` is the counter-clockwise flank (tooth 0's +y side) |
| `pitch_diameter(module, teeth, *, helix_angle=0)`, `base_diameter(…, pressure_angle=20, helix_angle=0)`, `tip_diameter(…, profile_shift=0, helix_angle=0)`, `root_diameter(…, profile_shift=0, clearance=0.25, helix_angle=0)` | float | `m z / cos β`, `d cos α_t`, `d + 2 m (1 + x)`, `d − 2 m (1 + c − x)` |
| `tooth_thickness(module, *, pressure_angle=20, profile_shift=0, backlash=0, helix_angle=0)` | float | `m_t (π/2 + 2 x tan α_n) − backlash` on the pitch circle |
| `centre_distance(module, teeth1, teeth2, *, shift1=0, shift2=0, pressure_angle=20, helix_angle=0)` | float | `m_t (z1 + z2) / 2`, or with shifts through the working pressure angle `inv α_wt = inv α_t + 2 (x1 + x2) tan α_n / (z1 + z2)`; a negative count is an internal gear (`centre_distance(m, 12, -36)`), shifts on an internal pair refused |
| `mesh_turn(teeth)` | float | half a pitch for an even count, 0 for an odd one: the turn that puts a wheel's space on the line of centres when the pinion's tooth 0 points at it |
| `min_teeth(*, pressure_angle=20, profile_shift=0, clearance=0.25, fillet=0.38, helix_angle=0)`, `auto_shift(teeth, *, …)` | float | the undercut limit `2 ((1 + c − x) − fillet (1 − sin α)) / sin² α` (17.1 at 20°), and the smallest shift that puts `teeth` above it |
| `planetary(sun, planet, *, planets=3, module=1, helix_angle=0)` | dict | `ring = sun + 2 planet`; `ratio_fixed_ring = 1 + ring/sun`, `ratio_fixed_carrier = −ring/sun`, `ratio_fixed_sun = 1 + sun/ring`; `centre_distance` (`m (sun + planet) / (2 cos helix_angle)` — the transverse module's, so a herringbone set passes its helix angle here as it does to the gears); `planet_angles`, `planet_turns` and `ring_turn` (the phases that mesh every planet with the sun and the ring at once; they do not depend on the helix). Refuses when `(sun + ring) % planets != 0` — the planets could not be spaced equally — naming the counts that would, or when neighbouring planets' tips collide |
| `planetary_set(module, teeth_sun, teeth_planet, width, *, planets=3, helix_angle=30, pressure_angle=20, gap=0.35, layer=0.2, layers_up=2, bore_clearance=0.35, pin=8, plate=3, rim=4, rim_gap=0.5, socket=3.5, foot=0.3, pin_overlap=0.5, head_flare=1, head_rise=1.2, head_height=1.5, ring_trim=0.02, tolerance=0.01, sun_boss=None, ring_extra=None, plate_extra=None, carrier_extra=None, ring_lift=0, name=None)` | dict | the print-in-place herringbone planetary of `examples/planetary.star`, as the library builds it: every external gear thinned by `gap / (cos α_t cos β_b)`, the ring shifted by `b / (2 m_t tan α_n)` and thinned by `ring_trim`, the root clearance `max(0.25, gap / module)`, the sun and the planets `layers_up` layers over the carrier plate, headed pins at 39.8°, every body at `tessellation(tolerance=)`; each body is a `part` and what comes back is instances: `sun`, `planets`, `ring`, `carrier`, `instances` (carrier, ring, planets, sun), the part bodies `sun_body`/`planet_body`/`ring_body`/`carrier_body`, the arithmetic (`P`, `a`, `teeth_ring`, `m_t`, `alpha_t`, `beta_b`, `backlash`, `ring_shift`, `clearance`, the three `layout`s) and the placement (`z_gears`, `ring_height`, `ring_phase`, `r_plate`, `r_pin`, `r_bore`, `r_boss`, `pin_height`, `pin_centres`). Hooks take `(body, info)` — `sun_boss(z_top, info)` the boss to glue on the sun, `ring_extra` the ring gear before it is phased, `plate_extra` the carrier plate before its pins (glue raised figures here, on the pristine cap), `carrier_extra` after them; `ring_lift` shortens the ring gear and lifts it so something else stands on the bed under it. `examples/planetary.star` and `examples/gear_toy.star` are the two consumers; the numbers they measure are the same because the construction is |

Faces: the caps `<name>/start` (z = 0) and `<name>/end` — the extrude's and
the loft's own names (§4.4); on a herringbone `<name>_lower/start` and
`<name>_upper/end` — and `<name>/side[0.<s>]` for outline
segment `s` (counted round loop 0 from the root land before tooth 0; tooth
`i` owns `i * segments` up to `(i + 1) * segments`), `<name>/side[1.*]` for
the bore or the housing circle. Refused, with the sentence naming the
number: a root fillet that does not fit between the teeth (the largest that
fits is quoted, in modules), a tooth pointed below its tip circle, a bore
outside the root circle, a rack thinner than its own tooth depth, a
herringbone without a helix angle, a hand that is not `right` or `left`.

```python
load("@cadpole/gears.star", "spur", "internal", "herringbone", "centre_distance", "layout", "tooth_faces", "planetary")
pinion = spur(2.5, 17, 8, bore=8, backlash=0.1, name="pinion")
ring = internal(2.5, 41, 8, 4, backlash=0.1, name="ring")
pinion = pinion.translate((centre_distance(2.5, 17, -41), 0, 0), name="pinion_placed")   # tooth 0 into space 0
flanks = tooth_faces(pinion, layout(2.5, 17, backlash=0.1), 0)
paint(pinion, faces=flanks["flank_left"] | flanks["flank_right"], color="#d0342c", name="marked")
P = planetary(12, 12, planets=3, module=1.5)          # ring 36; (12 + 36) % 3 == 0
sun = herringbone(1.5, 12, 12, 30, name="sun")
```

`examples/gears.star` is the worked example, `examples/planetary.star` and
`examples/gear_toy.star` the two that share `planetary_set`;
`stdlib/gears_test.star` holds
the correctness tests (the closed forms; the outline's tooth thickness read
back from its edges; two spurs at the centre distance standing `b cos α`
apart; a rack on a pinion; a 12/18/48 planetary meshing at `planetary()`'s
phases with the backlash asked for; the herringbone halves mirror images;
a sweep over 6 to 120 teeth).

---

## P6. Springs, threads and fasteners: `@cadpole/mechanical.star`

```python
load("@cadpole/mechanical.star", "spring", "thread", "bolt", "nut",
     "iso_profile", "square_profile", "acme_profile", "thread_depth", "minor_diameter")
```

| Function | What it builds |
| --- | --- |
| `spring(wire_d, coil_d, pitch, turns, *, hand="right", ends="open"\|"ground", start=, kernel=, name=)` | a round wire swept along `helix()` under `sweep_frame.rmf`; `ends="ground"` cuts it flat at both ends |
| `thread(major_d, pitch, length, *, profile="iso"\|"square"\|"acme"\|dict, internal=False, hand=, clearance=0, start=, phase=0, overrun=None, tolerance=None, kernel="mesh", name=)` | a threaded rod (the groove swept helically under `sweep_frame.frenet` and subtracted from the major cylinder), or with `internal=True` the body to subtract for a threaded hole: the minor bore plus the swept ridge, offset outward by `clearance` on every surface |
| `bolt(major_d, pitch, length, head_af, head_h, *, thread_length=None, profile=, hand=, start=, phase=, tolerance=, kernel=, name=)` | a hex-head bolt, tip at `start`, thread from the tip |
| `nut(major_d, pitch, af, height, *, clearance=0.1, profile=, hand=, start=, phase=, tolerance=, kernel=, name=)` | a hex nut whose thread clears a coaxial bolt's by `clearance`, which `distance(bolt, nut)` measures within the chord contract |
| `iso_profile(p)`, `square_profile(p)`, `acme_profile(p)` | the groove of one pitch as `{"depth", "top", "bottom", "angle"}`; `thread_depth(p, profile)`, `minor_diameter(d, p, profile)` |

Every thread on an axis is phased by height, like a thread cut from stock —
a right-hand groove's centre crosses +X at `z = 0, p, 2p, …` — so a bolt and
a nut built about the same axis mate at any height. The groove sits 0.05 mm
proud of the major cylinder with its flanks continued and its helix runs
half a pitch past each end, so the only faces that cross the shank are the
flanks, at the flank angle: the tangent case is avoided by
construction. The boolean is hundreds of NURBS faces against a cylinder; the
exact kernel's classifier names the helix crossing the end faces at the
helix angle (an edge-on-face contact) and refuses, so `kernel=` defaults to
the mesh kernel and the result is a mesh body. `examples/spring_and_bolt.star`
is the worked example; `stdlib/mechanical_test.star` its tests.
