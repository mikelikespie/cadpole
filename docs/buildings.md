# Buildings and plans

Building work in Cadpole: the massing of a house, its lot and the ground it
stands on, its floor plans, the shadow it throws and what it hides, and the
drawing set. This document is what that work adds to the shared core, and
all of it is optional: a script that models a part never loads or calls any
of it. For mechanical work see [`parts.md`](parts.md).

There is no switch between the two kinds of work. A building is the same
bodies as everything else: a floor is an extrusion, a roof is a body, the
kernel is still millimetres, and parameters, tests, layers, drawings, sheet
sets and exports work as they do for a part
([`starlark-api.md`](starlark-api.md)). The sun, sightline and `opt` globals
are available to any script; they are documented here because buildings are
what they are for.

A section reference is written §n: plain numbers are sections of
`starlark-api.md`, §B numbers are sections of this document and §P numbers
are in `parts.md`.

| Section | What is in it |
| --- | --- |
| [§B1](#b1-what-it-is-for) What it is for | the work and the examples |
| [§B2](#b2-a-first-building) A first building | a script to run and change |
| [§B3](#b3-commands-and-units) Commands and units | the CLI for buildings; feet and inches |
| [§B4](#b4-feet-inches-and-massing-cadpolebuildingstar) Feet, inches and massing | `@cadpole/building.star`: `ft`, `inch`, levels, floors, roofs, decks, stairs, floor areas, height envelopes |
| [§B5](#b5-terrain) Terrain | `terrain`, contours, slabs |
| [§B6](#b6-lots-and-site-data-cadpolesitestar) Lots and site data | `@cadpole/site.star`: parcels, setbacks, envelopes above grade, cut and fill |
| [§B7](#b7-sun-and-shadow) Sun and shadow | `sun`, `solstices`, `equinoxes`, `shadow_study` |
| [§B8](#b8-sightlines-and-views) Sightlines and views | `sightline`, `sightlines`, `view_cone` |
| [§B9](#b9-choosing-values-the-opt-namespace) Choosing values | the `opt` namespace: linear and mixed-integer programs |
| [§B10](#b10-floor-plans-cadpoleplan) Floor plans | the `@cadpole/plan` modules: the plan value, drawing, audits, furnishing, solving, framing, partis |

## B1. What it is for

| Work | Example | Where |
| --- | --- | --- |
| A floor plan read as a plan sheet, a section, an elevation and a 3D model | `examples/cabin_plan.star` | §B10 |
| Massing on a lot: levels, roofs, decks, stairs, floor areas | `examples/cabin_site.star`, `examples/hillside_lot.star` | §B4 |
| The ground: a survey, contours, an envelope above grade, cut and fill | `examples/hillside_lot.star`, `examples/site_survey.star` | §B5, §B6 |
| Shadows on the neighbours and what a house hides | `examples/shadow_study.star` | §B7, §B8 |
| Rooms sized and placed by a solver from stated rules | `examples/room_layout_opt.star`, `examples/plan_parti_compare.star` | §B9, §B10 |
| A plan checked against residential code rules, with the failures marked on the sheet | `examples/plan_toy_house.star` | §B10 |
| A multi-page drawing set | `examples/house_set.star`, `examples/color_plan.star` | §6.4 |
| A building printed one storey to a plate | `examples/storey_kit.star`, `examples/story_kit.star` | §6.1, §6.2 |

Every building, lot and rule set in the examples is invented.

## B2. A first building

```python
units(units.ft_in)
load("@cadpole/building.star", "ft", "floor", "gable_roof")
w = param("width", 24, unit="ft", min=16, max=40)
plate = sketch(XY).rect(ft(w), ft(16))
walls = floor(plate, 0, ft(9), name="walls")
roof = gable_roof(plate, pitch=0.5, overhang=ft(1), eave_z=ft(9), name="roof")
```

Save it as `house.star`, then:

```
cadpole render house.star -o house.png
cadpole inspect house.star              # the bodies, in feet and inches
cadpole serve house.star                # the web UI, with a width slider in feet
```

`examples/cabin_plan.star` is the next step: rooms as rectangles in feet,
from which the walls, doors, plan sheet, section, elevation and 3D model
follow. Its kitchen, bath and bedrooms are furnished by the room stage
(`furnish()`, §B10): the answer is written into the script, so it evaluates
without a solver, and a test solves the rooms again and compares. Its 3D
model is cut like a plan: the walls, glass and tall cabinets above
`plan_cut` (4 ft) are on the `above_cut` layer, so `--layer roof=off
--layer above_cut=off` shows the furnished rooms from above.

## B3. Commands and units

**Units.** The kernel is millimetres, as everywhere (§1). A building script
writes lengths through `ft()` and `inch()` (§B4), which return millimetres,
and declares `units(units.ft_in)` (§4.1) so that everything written back
(`cadpole inspect`, `distance()`, dimensions on drawings, the web UI) reads
in feet and inches. `param("width", 24, unit="ft")` is a slider in feet. A
plan value (§B10) and an `opt` problem (§B9) hold plain numbers in feet. A
script drawn in plain feet with no `units(...)` of its own takes
`--model-unit ft` on `cadpole export`, `cadpole bake` and `cadpole publish`,
which scales it to millimetres once.

```
cadpole test examples/cabin_plan.star                    # the plan's own tests
cadpole export examples/house_set.star --run-exports --out-dir .          # a four-sheet set as one PDF
cadpole export examples/color_plan.star --view top --section z=1200 -o plan.pdf   # one vector plan
cadpole render examples/shadow_study.star --shadow-study --sun 38,-121,2026-06-21,12:00,-8 -o study.png
                                                         # nine tiles: three dates by three times
cadpole test examples/room_layout_opt.star               # a layout solved inside the evaluation
cadpole solve examples/plan_parti_compare.star -p refit=cross_stair       # a long solve, run as a job
cadpole jobs                                             # the solve jobs on this machine
cadpole publish examples/cabin_site.star --model-unit ft --out-dir cabin  # a read-only web page, in feet
cadpole export examples/storey_kit.star --run-exports --out-dir kit       # a print project, a storey to a plate
cadpole check examples/story_kit.star --layer kit --max-bridge 20         # can the pieces be printed
```

Solving needs native Cadpole (the CLI, `cadpole serve`, `cadpole mcp`); the
browser's engine builds an `opt` problem and stops at `solve()` (§B9).

---

## B4. Feet, inches and massing: `@cadpole/building.star`

```python
units(units.ft_in)
load("@cadpole/building.star", "ft", "inch", "ft_in", "pitch", "level", "floor",
     "hip_roof", "gable_roof", "shed_roof", "cupola", "deck", "stair",
     "garage_door", "elevator_shaft", "floor_area", "gross_floor_area",
     "lot_coverage", "footprint_of", "straight_skeleton", "roof_profile",
     "height_envelope", "height_check", "floor_area_cap")
```

Architecture massing. Every length is a millimetre, as everywhere:
`ft(45)` is `45 * 304.8`, a plain number, so `ft(45) + inch(6)` just works;
how it is *written back* is the document's `units(...)` (§4.1), not the
library's business. A **footprint** is a Sketch on XY with one loop or a
list of `(x, y)` points, read counter-clockwise whichever way it was drawn.
**Pitch** is rise over run as a plain number (`pitch(6)` is 6:12, `pitch("6:12")`
the same, `pitch(30, "deg")`). Code limits are parameters, and a refusal is an
error that names the number. The module's docstring carries the
constructions; this is the API.

| Call | Returns | |
| --- | --- | --- |
| `mm(x)`, `cm(x)`, `m(x)`, `inch(x)`, `ft(x, inches=0)`, `ft_in(feet, inches)` | float | millimetres: `ft(45, 6)` and `ft_in(45, 6)` are 13868.4 |
| `sq_ft(area_mm2)` | float | an area in square feet, for a figure zoning reads |
| `pitch(rise, run=12)` | float | `pitch(6)` → 0.5; `pitch("6:12")`; `pitch(30, "deg")` |
| `polygon_area(points)`, `offset_polygon(points, distance)` | float, list | plan arithmetic on point lists: the area whichever way round; every edge moved outward by `distance` (inward when negative) with mitred corners — right for any simple polygon whose edges are longer than the offset (an eave overhang, a slab inset); `Sketch.offset` is the general case |
| `straight_skeleton(points)` | dict | the straight skeleton of a simple polygon by Aichholzer's wavefront: `arcs` (`(a, b, t_a, t_b, (edge_i, edge_j))`), `faces` (one `(x, y, t)` polygon per edge), `nodes`, `height` (the ridge, in units of distance; at pitch `p` a point at time `t` is `p t` high), `points`. Edge, split and collapse events all handled; a rectangle's two ends and a square's four are taken one at a time at the same instant |
| `hip_roof(footprint, *, pitch, overhang=0, eave_z=0, fascia=0, ridge=None, kernel=None, name=None)` | Body | every eave edge's plane at `pitch`, meeting on the skeleton. An exact `loft` on a single-apex footprint (a square, a regular polygon); otherwise a **mesh body of the skeleton's planes**: on a convex footprint an oversized prism cut by one half-space per eave on the mesh kernel (the closed-form volume `W h (3L − W) / 6` on a rectangle, to the mesh's f32, five named faces), past a reflex corner one piece per skeleton face, the pieces overlapping 0.5 mm across the arcs, united on the mesh kernel (a ridge shared by four faces is what the exact glue refuses). `kernel="brep"` asks for the exact cuts: they close on many rectangles and panic in truck on others (30 × 15 ft at 6:12), which the native kernel retries on the mesh kernel and the wasm engine cannot survive — a native-only choice, never the default. `ridge=(p0, p1)` on a four-sided footprint places the ridge in plan (the end planes then pass through the end eaves and the ridge ends; a saltbox when off centre). `fascia` is the vertical face at the eave. Names `<name>`, `<name>_blank`, `<name>_outline`, `<name>_cut<i>`, `<name>_tool<i>`, `<name>_face<k>`, `<name>_piece<k>`, `<name>_trim` |
| `gable_roof(footprint, *, pitch, ridge_axis=None, overhang=0, eave_z=0, fascia=0, kernel=None, name=None)` | Body | two planes to a ridge along `ridge_axis` (`"x"`/`"y"`, the longer side by default): one exact `extrude` of the triangle on an axis-aligned rectangle; elsewhere the prism cut by the two planes, exact with a `fascia`, a mesh body without one (the cut is then tangent to the prism's edge) |
| `shed_roof(footprint, *, pitch, direction=(1, 0), overhang=0, eave_z=0, fascia=0, kernel=None, name=None)` | Body | one plane rising along `direction` from its low eave; an exact extrude on an axis-aligned rectangle with an axis direction, else the prism cut by the plane (exact with a fascia, mesh without) |
| `cupola(width, height, *, on, at=None, pitch=1.0, sink=None, depth=None, name=None)` | Body | a box with a pyramid roof straddling roof body `on`'s ridge, its floor `sink` (a foot) below it |
| `level(name, elevation, height, *, bodies=None, plan_cut=None)` | dict | a storey as data: `name`, `z`, `height`, `top`, `bodies`, `plan_cut` (4 ft); adds nothing to the model |
| `floor(footprint, z, height, *, solid=True, slab=None, walls=None, name=None)` | Body | the footprint extruded `z` to `z + height`: a box, or with `solid=False` a slab (`slab` thick, a foot) with `walls` (6 in) round it, open at the top, one exact body |
| `deck(footprint, z, thickness, *, posts=None, rail=None, ground=0, name=None)` | dict | the slab, `posts=(spacing, size)` at every corner and at most `spacing` apart, from `ground` (a height or a function `(x, y) → z`, a terrain's) to the underside, `rail=(height, thickness)`; `slab`, `posts`, `rails`, `bodies` |
| `stair(from_z, to_z, width, *, run=None, rise=None, landings=None, code="irc", direction=(1, 0), at=(0, 0), name=None)` | dict | a solid wedge of steps: the fewest risers that keep every rise at or under `code`'s (IRC R311.7.5: 7 3/4" rise, 10" run, 36" wide; `"none"` lifts them; a dict of `max_rise`/`min_run`/`min_width` is its own code), `landings=[k, …]` a landing `width` deep after step `k`; `body`, `risers`, `rise`, `run`, `length`, `top`, `width`. Refused, naming the number: a rise over the maximum, a run under the minimum, a width under the minimum |
| `garage_door(mass, width, height, *, at, facing=(0, -1), recess=None, name=None)` | Body | a recessed panel cut `recess` (4 in) into the face of `mass` whose outward normal is `facing` |
| `elevator_shaft(width, depth, from_z, to_z, *, overrun=None, pit=None, at=(0, 0), name=None)` | Body | one box from `from_z − pit` (4 ft) to `to_z + overrun` (42 in) |
| `floor_area(body, z, *, cut=None, exclude_under=None, name=None)` | float | mm² of the plan cut `cut` (4 ft) above the level, restricted to where the ceiling is at least `exclude_under` (5 ft) above it — `section(z + cut) ∩ section(z + exclude_under)`, the rule zoning applies to a room under a sloped ceiling, computed on the geometry. A box mass gives its footprint; an attic under a hip roof the part a person can stand in; 0 where the body has no material |
| `gross_floor_area(levels, *, cut=None, exclude_under=None)` | dict | `floor_area` of every body of every level, summed: `levels` (`[{"name", "area"}]`), `by_level`, `total` |
| `footprint_of(bodies, *, name=None)`, `lot_coverage(bodies, lot)` | Sketch, dict | the union of the bodies' plan footprints (each cut just above its lowest point); `area`, `lot_area`, `ratio` |
| `roof_profile(body, *, axis=(1, 0), through=None, step=None)` | list | the top of `body` along a line in plan as `(v, z)`, `v` the distance along `axis` from `through` (the plan centre by default), from a vertical section: a point every `step` (6 in) and one at every vertex of the section, a vertical edge (a parapet, a setback) as two points at one `v`, so the body's own breakpoints are exact |
| `height_envelope(grade, roof, *, limit=None, cap=None, rear=None, step=None)` | dict | the height-over-grade envelope **with averaging**, as hillside height rules measure it, below |
| `height_check(grade, roof, *, limit=None, step=None)` | dict | the plain check: `passes` when no point of the roof is more than `limit` (30 ft) over the grade under it; `max_over`, `max_over_at`, `limit_line`, `samples` |
| `floor_area_cap(lot_width, lot_depth, *, front_setback, rear_yard, floors, deduction=None, built=None)` | dict | the floor-area cap of a buildable envelope: the **plate** is the lot between the front setback and the rear-yard line at the lot's full width, the **envelope** the plate times `floors`, the **cap** the envelope less `deduction` (650 sq ft); `built` is a list of areas (mm²) or of `gross_floor_area`'s `levels` rows. `plate`, `envelope`, `cap`, `built`, `remaining`, `ratio`, `passes` (`built <= cap`) |

**The height envelope.** `grade` and `roof` are `(v, z)` profiles along the
same axis — the lot depth, `v` from the front, in millimetres — or `roof` is
a body, read along x through its plan centre by `roof_profile` (call it for
another line). The limit line is `grade + limit` (30 ft) at every point. The
building **passes** when the area of its profile above that line is no more
than the area of the line's envelope it leaves unbuilt below it
(`over_area <= under_area` — the averaging rule: a taller front pays for a
lower back), no point exceeds `cap` (40 ft) over grade, and, with
`rear=(depth, height)` (for example `(ft(8), ft(32))`), no point in the
rearmost `depth` of the building exceeds `height` over grade. Both profiles
are piecewise linear, so their difference is too, and the two areas are
integrated exactly between the union of their breakpoints — a vertical step
(a parapet, a setback) is two breakpoints at one `v` and contributes nothing,
where a sampled sum would put half a step's worth on the wrong side. Returns
`passes`, `over_area`, `under_area` (mm²), `max_over`, `max_over_at`,
`cap_ok`, `rear_ok`, and the lines to draw — `limit_line`, `cap_line`,
`rear_line` as `(v, z)` lists every `step` (6 in) — plus `samples`
(`(v, grade, roof, limit)`).

```python
units(units.ft_in)
load("@cadpole/building.star", "ft", "inch", "height_envelope", "height_check", "floor_area_cap", "sq_ft")
grade = [(0, 0), (ft(50), 0)]                                        # a flat lot, 50 ft deep
roof = [(0, ft(35)), (ft(20), ft(35)), (ft(20), ft(25)), (ft(40), ft(25))]   # 35 ft over the front 20 ft, 25 ft over the back 20 ft
env = height_envelope(grade, roof, rear=(ft(8), ft(32)))
assert_true(env["passes"])                      # 100 sq ft over the 30 ft line, 100 sq ft under it: averaging passes
assert_true(not height_check(grade, roof)["passes"])   # the plain check does not: 5 ft over at the front
top = env["limit_line"]                          # (v, z) every 6 in: draw it as a 2 in ribbon on the section plane
ribbon = extrude(sketch(XZ).polygon(top + [(v, z - inch(2)) for v, z in reversed(top)]), inch(1), name="limit_line")
cap = floor_area_cap(ft(25), ft(100), front_setback=ft(15), rear_yard=ft(25), floors=3, built=[ft(1) * ft(1) * 1500, ft(1) * ft(1) * 1500, ft(1) * ft(1) * 800])
assert_true(cap["passes"])                      # 60 x 25 ft plate, 4,500 over three floors, 3,850 after the deduction; 3,800 built
print(sq_ft(cap["remaining"]))                  # 50
```

`stdlib/building_test.star` holds the tests (the units; the skeleton of an L,
a rectangle, a square and a T; every roof's closed-form volume, with pitch
sweeps; the floors, decks, stairs, doors and shafts; the floor areas and the
cap; the envelope with and without averaging, on a slope, against the rear
strip and from a gable body), and `cadpole_script_test`'s
`the_building_library_refuses_by_number` the refusals.
`examples/hillside_lot.star` is the two site libraries together: the
`(depth, z)` pairs of `@cadpole/site.star`'s `grade_profile` are the `grade`
of `height_envelope`, the house's `roof_profile` down the centreline the
roof, and the document declares `units(units.ft_in)`.

---

## B5. Terrain

A terrain is a ground surface `z = f(x, y)`: built from a survey's spot
elevations, a LiDAR footprint's ground samples or a topo sheet's contour
lines; asked for the height, the slope and the contours; raised by a
height for a zoning envelope; cut to a lot's rectangle; and made a slab
body that booleans with a massing. Two constructions, one value (§3
`Terrain`): a **TIN** — the Delaunay triangulation of the points, through every one, the
surface a surveyor's software draws, whose body is a watertight mesh — and
a **fit** — a least-squares bicubic B-spline through the same points (P&T
§9.4.3 on a clamped uniform knot vector, about two points per control
point, the least-squares plane taken out first and put back into the net
so an empty span continues the plane, a `1e-8` second-difference penalty
so no span can swing) whose body is an exact B-rep with the patch on top,
so the envelope stays on the exact path and exports to STEP. Use the TIN
for what was measured (earthwork, the contours the plan shows, a shot read
back exactly); use the fit for what a rule is measured *against* (an
envelope, a height check), where a smooth surface is what the planner
means by "grade" and an exact body is what a boolean wants.

#### `terrain(points, *, method=terrain.tin, extent=None, name=None)` → Terrain

| Arg | Type | Notes |
| --- | --- | --- |
| `points` | list of `(x, y, z)` | spot elevations, mm; at least 3 not collinear, at most 200,000. A TIN drops a point at the same plane position as an earlier one (the first's `z` stands) and says how many; a fit uses every point inside the extent |
| `method` | `terrain.tin` / `terrain.fit`, or the string (§4.16) | |
| `extent` | `((x0, y0), (x1, y1))` or `(x0, y0, x1, y1)` | the rectangle the surface covers. Without it a TIN covers the points' convex hull and a fit their bounding rectangle. With it a TIN is clipped to the rectangle — its corners interpolated from the points, or continued from the plane of the twelve nearest where the data does not reach, points outside left out and counted — and a fit is defined over it, continuing its least-squares plane where no point reaches |
| `name` | str | the feature id; the bodies are `terrain_body` features of their own |

The feature records `method`, `points`, `extent`, and a TIN's `triangles`
(with `duplicates` and `outside` when non-zero) or a fit's `control`,
`spans`, `rms_residual` and `max_residual`; `cadpole inspect` prints the same
as an info line: `terrain: a TIN of 64 points in 122 triangles over
(-7315.2, -3048) .. (21336, 29870.4)` and `terrain: a bicubic B-spline
through 60 points on a 5×5 control net (2×2 knot spans) over (-7315.2,
-3048) .. (21336, 29870.4): rms residual 91.66 mm, largest 226.98 mm`
(the hillside example: the 60 shots plus the extent's four corners; the
fit's residual is inches on a survey whose shots are a foot apart in
elevation).

#### `terrain_from_contours(polylines, *, method=terrain.tin, extent=None, spacing=None, name=None)` → Terrain

`polylines` is a list of polylines, each a list of `(x, y, z)` with `z` the
line's elevation, as a contour drawing or a DXF import gives them. Every
vertex is a point, and any segment longer than `spacing` (default: the
median segment length) is sampled at that spacing so the surface follows
the line between its vertices; the points then go through `terrain(...)`
with the same `method`, `extent` and `name`, and the feature records
`polylines` and `spacing` besides. A TIN of contour points has the usual
flat triangles where a ridge or a valley crosses between lines;
`method=terrain.fit` smooths them.

```python
load("@cadpole/site.star", "ft", "to_mm", "local_frame", "local_to_latlon", "parcel",
     "profile_points", "rear_yard", "setbacks", "envelope_above_grade", "cut_fill")

frame = local_frame(38.0, -121.0, rotation_deg=15, units="ft")         # the lot upright in its own frame
lot = parcel(frame, corners_as_latlon, front=0, name="lot")             # 40 x 80 ft; lot["lines"]["rear"], lot["depth"]
# A surveyed profile down the lot and LiDAR ground samples either side, in the frame's feet.
points = profile_points(lot, to_mm(frame, PROFILE)) + to_mm(frame, LIDAR)
grade = terrain(points, method=terrain.tin, extent=EXTENT, name="grade")        # through every shot
grade_fit = terrain(points, method=terrain.fit, extent=EXTENT, name="grade_fit")  # smooth, exact

ground = grade.body(base=-ft(35), name="ground")                       # the site, a mesh slab
z = grade.z_at(ft(20), ft(40)); s = grade.slope_at(ft(20), ft(40))    # 26 % here
plan = grade.contour_sketch(ft(1), name="contours")                    # 1 ft contours for the site plan
yard = rear_yard(lot, ft(52.5), name="rear_yard")                      # the plan area in front of the rear-yard line
envelope = envelope_above_grade(grade_fit, yard, ft(30), cap=ft(40), base=-ft(30), name="envelope")
#   = grade_fit.offset_up(ft(30)).clip(yard rectangle).body(base=-ft(30)): one exact B-rep, no boolean
envelope.material(color=(0.95, 0.75, 0.25), opacity=0.3, shadow=False)   # guide geometry: seen, casting no shadow
pad = cut_fill(grade, setbacks(lot, front=ft(2), rear=ft(25), sides=ft(0.5)), -ft(10), name="pad")
#   pad["cut"], pad["fill"], pad["cut_volume"], pad["fill_volume"]
fits = intersect(house, envelope_from_the_tin).volume() > 0.998 * house.volume()
```

Rules:

- **A height field, always.** Both surfaces are `z = f(x, y)` over a
  rectangle or a convex region: `z_at` and `slope_at` are defined on it, an
  overhang is not a terrain, and the fit's control net has its `x` and `y`
  at the Greville abscissae, so the patch is exactly `x = x(u)`, `y = y(v)`
  and its four boundary curves lie in the extent's four vertical planes —
  which is what lets the slab's sides be planar faces.
- **Offset is a shift.** "Thirty feet above grade at every point" is the
  surface raised thirty feet in `z`: every vertex, or every control point,
  moved. Exact in both cases and a boolean-ready body in both; not a
  normal offset, because the rule is written in elevation.
- **A rectangle comes out exactly.** `clip(extent)` is the same surface
  over a smaller rectangle: a fit split at the rectangle's parameters by
  knot insertion, a TIN's triangles clipped with every new vertex on an
  old edge (a crossing computed once, from the edge's endpoints in a fixed
  order, so the two triangles that share the edge share the vertex to the
  bit). That is how a lot's envelope is a slab over the lot with no
  boolean: the exact kernel's intersection of the patch with a prism leaves
  a trimmed face the tessellator cannot always close, and a mesh
  boolean tessellates the patch at the document tolerance whatever the
  body's own `tessellation()` says.
- **A site is large.** At the document's 0.05 mm tessellation a fitted slab
  the size of a lot is hundreds of thousands of triangles; `body(tolerance=)`
  (and `envelope_above_grade`'s ¼ in default) sets the slab's own. The TIN's
  triangles are the surface and cost nothing more.
- **One kernel op per body**, keyed by the points, the method, the extent,
  the clip, the offset and the base: a re-run that moves nothing
  rebuilds nothing, and a sweep over a setback re-evaluates the envelope
  once.
- **Refused by name:** fewer than three points or collinear ones; a
  non-finite point or extent; an extent of no width; a base at or above
  the lowest point; a clip outside the extent; a contour interval that is
  not positive, or more than 500 lines; a query off the surface (naming
  the terrain and its extent); a fit whose normal equations are not
  positive definite (it cannot happen with the penalty, and says so if it
  does); a method that is not `tin` or `fit`.

Not implemented: breaklines on the TIN, a surface clipped to an arbitrary
polygon (a rectangle is exact; anything else is a boolean with the slab), a
normal offset, datum conversion (a number the script adds) and reading a
DEM raster.

---

## B6. Lots and site data: `@cadpole/site.star`

```python
load("@cadpole/site.star", "ft", "inch", "to_ft", "to_mm",
     "local_frame", "latlon_to_local", "local_to_latlon",
     "parcel", "setbacks", "rear_yard", "lot_point", "profile_points", "grade_profile",
     "envelope_above_grade", "cut_fill", "footprint_mass", "polygon_area")
```

The site layer over §B5's `terrain()`: the shapes site data comes in
— parcel corners as latitude and longitude, a surveyed profile down the
lot, LiDAR ground samples and neighbouring footprints — turned into a
lot on XY, a ground surface, an envelope and earthwork. Every body-making
function takes millimetres (`ft(45)` is 13716) and forwards `name=`; a
frame carries its own unit for what maps and surveys give. Records nothing
when loaded.

| Function | Returns | |
| --- | --- | --- |
| `ft(x)`, `inch(x)`, `to_ft(mm)` | float | feet and inches as millimetres, and back |
| `local_frame(lat0, lon0, rotation_deg=0, units="ft")` | dict | a tangent plane at `(lat0, lon0)`: x east, y north, turned counter-clockwise by `rotation_deg` so a lot stands upright; metres per degree from the WGS84 series (at 38° one arc-second is 101.16 ft north and 80.05 ft east); a site, not a county |
| `latlon_to_local(frame, lat, lon)`, `local_to_latlon(frame, x, y)` | (x, y) / (lat, lon) | in the frame's units, each the other's inverse |
| `to_mm(frame, points)` | list | `(x, y)`, `(x, y, z)` or `(depth, z)` pairs in the frame's units as millimetres |
| `parcel(frame, corners, front=0, latlon=True, name=None)` | dict | a lot from its recorded corners either way round: `sketch` (XY, counter-clockwise), `corners`, `lines` (`front`, `rear` — the edge deepest from the front — and `side[0]`, `side[1]`, … round from the front), `edge_names`, `faces` (the `side[0.k]` suffix each named side gets when the sketch is extruded), `front_index`, `rear_index`, `area`, `width`, `depth` |
| `setbacks(lot, front=0, rear=0, sides=0, name=None)` | dict | the lot clipped by each side's line moved inward: `sketch`, `corners`, `lines` (the setback lines, named as the sides are), `area`; refuses setbacks that close it, naming the line |
| `rear_yard(lot, depth, name=None)` | dict | the line parallel to the front `depth` in from it (a code's "45 % of the lot depth") and the plan area in front of it, as `setbacks` returns it, plus `line` and `depth` |
| `lot_point(lot, across, depth)` | (x, y) | the site point `across` along the front and `depth` in from it: the lot's own coordinates on the site's |
| `profile_points(lot, profile, across=None)` | list | a surveyed `(depth, z)` profile placed at every `across` position (default both side lines and the centreline) as `(x, y, z)` points for `terrain()`; add the neighbours' samples and call `terrain(...)` |
| `grade_profile(terrain, lot, across=None, step=None, start=None, end=None)` | list | the ground along the depth as `(depth, z)` pairs at `across` (default the centreline) every `step` (1 ft): what a section plots and what a building library's height check takes as its grade |
| `envelope_above_grade(terrain, area, height, cap=None, base=None, tolerance=None, name=None)` | Body | "no higher than `height` above grade at every point, never above `cap`": the terrain raised by `height` over the plan `area` (a parcel/setbacks/rear_yard dict, a list of points, or a Sketch), standing on `z = base` (10 ft below the lowest grade). A rectangular area on the site's axes is `Terrain.clip(rect).body()` — no boolean, so a fit's envelope is one exact B-rep; any other area is the slab intersected with its prism. The cap is applied only where the raised surface reaches it, and a biting cap on a fit runs on the mesh kernel at the document tolerance. `tolerance` is a fit's chord tolerance (¼ in) |
| `cut_fill(terrain, area, level, base=None, name=None)` | dict | the earthwork for a level pad: `cut` (the ground above `level` within the area) and `fill` (the space between the ground and `level` where the ground is below it), each a Body or `None` where there is nothing to do, `cut_volume`, `fill_volume`, `ground`; mesh booleans, so pass the TIN |
| `footprint_mass(frame, polygon, ground_z, height, latlon=True, name=None)` | Body | a neighbour: its footprint extruded `height` up from `z = ground_z` — a LiDAR footprint's ground elevation and median height |
| `polygon_area(points)` | float | signed, positive counter-clockwise |

`examples/hillside_lot.star` is the worked example (§B5);
`stdlib/site_test.star` its tests: the arc-second lengths, a parcel's names
either way round, setbacks by area, the rear-yard line, a profile placed and
read back, the envelope's volume on a 30 % slope from the TIN (a mesh) and
from the fit (exact, six named faces), a biting cap, cut and fill against
the closed forms, a neighbour on its ground. The datum trap: LiDAR is
NAVD88, a city benchmark is often a city datum; every elevation in one model is on one datum, shifted before
it goes in.

---

## B7. Sun and shadow

Where the sun stands for a place, a date and a time, and the shadow bodies
throw on a face under it. A render is lit by a sun through
`render_defaults(sun=)` (§4.12); the `Sun`, `Shadow` and `ShadowStudy` values
are in §3.

#### `sun(lat, lon, date, time, *, tz=None)`, `sun(*, az, el)`

Where the sun stands: for a place (degrees, north and east positive), a
date (`"YYYY-MM-DD"`) and a local clock time (`"HH:MM"`, or hours as a
number: `12.5`), `tz=` being the clock's offset from UTC in hours — the
longitude's standard zone when left out, with no daylight saving, so longitude
-121 is `-8`, or `-7` for a summer study on the summer clock. Or by
hand, `sun(az=210, el=45)`: compass azimuth (0 north, 90 east, clockwise)
and elevation. The answer is a `Sun` (§3). NOAA's solar position
algorithm (Meeus's low-precision solar coordinates and equation of time),
good to **half a degree** in azimuth and elevation and a minute on sunrise
and sunset; `.elevation` is apparent (refraction included), `.direction` a
unit vector toward the sun with `+y` north, `+x` east and `+z` up, which is
the model's own frame unless `render_defaults(north=)` or `--north` says
otherwise.

```python
s = sun(38.0, -121.0, "2026-12-21", "12:00", tz=-8)
print(s.azimuth, s.elevation, s.sunrise, s.sunset)     # 179.4 28.6 07:17 16:48
noon = sun(38.0, -121.0, "2026-06-21", 13, tz=-7).direction   # (0.024, -0.251, 0.968): x east, y north, z up
```

#### `solstices(year)`, `equinoxes(year)`

The dates (UTC) of the June and December solstices, and of the March and
September equinoxes, as `["YYYY-MM-DD", "YYYY-MM-DD"]` — the three days a
shadow study is drawn at (`solstices(2026) + equinoxes(2026)[:1]`). Meeus's
series, good to a minute over 2000–2100.

#### `shadow_study(bodies, *, on, sun=None, dates=None, times=None, lat=None, lon=None, tz=None, north=None)`

Where `bodies` (a body, a list, an instance or an assembly) throw their
shadow on a receiver, under one sun or several. `on=` is a planar face (a
neighbour's light court, a roof; the footprint is clipped to it), a plane
(unbounded; whichever side faces the sun receives), or a body (terrain:
its whole mesh, sampled). `sun=` is a `Sun` or a list of them; or `dates=`
and `times=` with `lat=`, `lon=` (and `tz=`) draw every combination.
`north=` as `render_defaults`. Returns a `ShadowStudy` (§3): one `Shadow`
per sun, in the order given (dates outer, times inner).

**Exactness.** On a plane or a planar face the footprint is the bodies'
triangles clipped to the sun's side of the plane, projected onto it along
the sun direction and unioned by a polygon boolean, then intersected with
the face's outline — the true silhouette extruded along the sun and cut by
the plane, with no sampling — so it is exact for the tessellation (the
geometry itself for a mesh body, a chord approximation within the
tessellation tolerance for an exact one). On terrain, every triangle that
faces the sun asks a spread of points (4096 over the receiver, shared out
by area, at least four each) whether a ray toward the sun meets a caster;
`.exact` is `False` and the area's error is the sampling's. A receiver
facing away from the sun is in its own shadow: `.lit` is `False` and the
footprint empty.

```python
court_top = court.faces(normal=(0, 0, 1)).one()
winter = solstices(2026)[1]
noon = shadow_study(house, on=court_top, sun=sun(LAT, LON, winter, "12:00", tz=-8))
assert_eq(noon[0].area(), 0, msg="the house adds no shadow on the light court at 12:00 on the winter solstice")

study = shadow_study(house, on=court_top, dates=solstices(2026) + equinoxes(2026)[:1],
                     times=["09:00", "12:00", "15:00"], lat=LAT, lon=LON, tz=-8)
worst = max([s.area() for s in study])
slab = extrude(study[0].sketch(name="june_morning"), 10)   # the footprint as a body, to render
```

`examples/shadow_study.star` is the worked example (a hillside lot, its
neighbours, the nine-tile study, the sidewalk sightline), and `cadpole render
--shadow-study` draws the same nine tiles as a contact sheet, or any list of
instants with `--at` (docs/views.md). In a script the list form is
`sun=[...]`: `shadow_study(house, on=court_top, sun=[sun(LAT, LON, winter,
"12:00", tz=-8), sun(LAT, LON, equinox, "09:00", tz=-8), sun(LAT, LON,
equinox, "15:00", tz=-8)])` is the winter solstice at noon and the equinox
morning and afternoon, one `Shadow` each, in that order.

---

## B8. Sightlines and views

Whether one point can see another past the model's bodies, how much of a
target an eye sees, and how much of a window of sky. They are measurements,
as `distance` is (§4.9), and return the `Sightline` and `SightlineFan`
values of §3.

#### `sightline(from, to, *, eye=None, obstacles=None)`

One line of sight from an eye point to a target point — a sidewalk to a
studio's lantern, a window to a landmark — through the bodies `obstacles=`
names (a body, a list, an instance or an assembly; every visible body when
left out; `through=` is accepted as a synonym). One ray through the
renderer's BVH (`render/bvh.rs`), on the bodies' tessellations, stopping at
the first body it meets. A body in the list is an obstacle whether or not
its material says `shadow=False`: the flag is the renderer's, and
a zoning envelope in the default set stops a sightline like any body, so
name the obstacles when the document has guide geometry. **`from` is a reserved word in Starlark** (the
lexer refuses `from=` anywhere, as it refuses `raise` and `in`), so the eye
point is the first positional argument or `eye=`; the target is the second
or `to=`: `sightline(sidewalk, lantern)` and `sightline(eye=sidewalk,
to=lantern)` are the same call. Returns a `Sightline`:

| Attribute | |
| --- | --- |
| `clear`, `blocked` | nothing stands between the two points / something does |
| `blocked_by` | the name of the first body the ray meets, or `None` |
| `blocked_face` | the named face it meets it on (§5): `house/right`, not a triangle index |
| `hit` | the point, or `None` |
| `distance` | mm from the eye to the blocker, or to the target when clear |
| `length`, `from`, `to` | the segment as asked |

```python
sidewalk = (50 * ft - passage * ft / 2, 0, 5.5 * ft)
line = sightline(sidewalk, lantern_top, obstacles=[house, studio])
assert_true(line.clear, msg="at %s ft the sidewalk sees the lantern: %s" % (passage, line))
```

#### `sightlines(from, target, *, eye=None, obstacles=None, samples=256)`

A fan of sightlines from an eye point to a target and the fraction of it
the eye can see: "how much of the studio's front does the sidewalk still
see with the passage at 8 ft rather than 12?"
(`examples/shadow_study.star`). `target` is a body, a face or a list of
points (positional or `target=`; the eye is positional or `eye=`, since
`from` is reserved); a body or a face is sampled by `samples=` points
spread over its surface in proportion to area (deterministically), and the
target itself joins the obstacles, so a point on its far side counts as
hidden by it. Returns a `SightlineFan`: `.fraction` (0..1, the rays that
reach their point before anything else; also `.visible`), `.blocked`,
`.rays`, `.visible_rays`, `.blocked_by` as `[(body, fraction), ...]` most
first, and `.target`. A sweep test over the parameter that opens the view
holds the fraction monotone (§7.1):

```python
def studio_front_seen_from_the_sidewalk():
    front = studio.faces(normal=(0, -1, 0)).one()
    return sightlines(sidewalk, front, obstacles=[house], samples=400).fraction

test("studio front from the sidewalk", studio_front_seen_from_the_sidewalk, sweep={"passage": [8, 12, 16]}, monotone=monotone.increasing)
```

#### `view_cone(from, *, eye=None, az, fov=60, el=(0, 30), obstacles=None, rays=32, range=None, north=None)`

How much sky a window sees: a grid of `rays` × `rays` rays from the eye
(positional or `eye=`) over a window of sky `fov` degrees wide about the compass azimuth `az`
(0 north, 90 east, clockwise) and between the elevation angles `el=(lo,
hi)` — the skyline band — through `obstacles=` (every visible body when
left out). `north=` says where north is in model space (degrees clockwise
from `+y`; default the document's `render_defaults(north=)`, else `+y`);
`range=` (mm) is how far a ray looks. Returns a `SightlineFan` whose
`.fraction` is the share of the rays that reach the sky and whose
`.blocked_by` says which body took the rest. Run it with and without the
proposal and the difference is the view it takes:

```python
before = view_cone(window, az=90, fov=120, el=(0, 25), obstacles=[studio], rays=24)
after = view_cone(window, az=90, fov=120, el=(0, 25), obstacles=[house, studio], rays=24)
assert_true(after.visible > 0.05, msg="the house leaves some of the eastern skyline: %s" % after.visible)
```

---

## B9. Choosing values: the `opt` namespace

Everything else in Cadpole checks a design (`test()`, `print_check()`, `sweep_table()`);
`opt` chooses one. A script declares variables, writes linear rules and an
objective, and asks for the best values; the answer says which rules bind,
what each costs, and -- when the rules cannot all hold -- which ones conflict.
It is a linear program (LP), or a mixed-integer one (MILP) as soon as a
variable is an integer: either/or rules, counts and yes/no choices, which is
what a floor plan needs ("this room is west of that one, or east, or ...").

**Solving needs native Cadpole**: the CLI (`cadpole eval`, `cadpole test`, `cadpole
render`, ...), `cadpole serve` and `cadpole mcp`, which link HiGHS (the solver
behind MATLAB's `linprog`/`intlinprog`, SciPy and JuMP). The browser's wasm
engine and the Apple apps build the same problem and stop at `solve()` with
"solving needs native Cadpole". Published sites are baked, so nothing in
a browser ever needs it.

```starlark
p = opt.problem(name="sizes")
w = p.var("w", min=8, max=20)            # a continuous variable
d = p.var("d", min=8, max=20)
p.constrain(w + d, max=30, name="perimeter")
p.maximize(2 * w + d)
sol = p.solve()
sol.value(w)          # 20.0
sol.binding()         # ["perimeter"]
sol.dual("perimeter") # 1.0: one more foot of perimeter is worth 1 of objective
```

| | |
| --- | --- |
| `opt.problem(*, name=None)` → `OptProblem` | A new problem; records a feature of kind `optimize` whose args carry its counts, the solver and the last answer's status and objective |
| `p.var(name, *, min=None, max=None, integer=False)` → `OptVar` | A variable. `None` is unbounded on that side, so write `min=0` for a size. Names are unique in a problem |
| `p.binary(name)` → `OptVar` | An integer from 0 to 1 |
| `p.constrain(expr, *, min=, max=, equals=, name=None, tags=None)` → name | `min <= expr <= max`, or `expr == equals`. A bound may be an expression (`p.constrain(a, max=b)` is `a <= b`); a range takes numbers. Unnamed rules are `constraint_<n>`. `tags=` is a dict from tag to string; with none, a name `family:floor:room:what` carries those three tags |
| `p.constraints(*, tags=None)` → names | The rules, in the order written: all, or those a tag selector picks -- `{"family": "apart", "floor": ["L1", "L2"]}` (every key matches one of its values), or a list of such dicts (any one) |
| `p.maximize(expr)`, `p.minimize(expr)` | The objective; one per problem, and a second names the first's line. None is a feasibility problem |
| `p.solve(*, time_limit=None, gap=None, max_nodes=None, count=1, distinct=None, fix=None, relax=None, drop=None, objective=True, start=None, job=False)` → `OptSolution` or a list | Solve as it stands. `count=` > 1: the best, then the best that differs from all before it in the binaries `distinct=` lists (every binary by default), stopping when there are no more. For this solve alone: `fix={var: number}` holds variables, `relax=[names]` and `drop=<tag selector>` leave rules out, `objective=False` asks for any feasible point, `start={var: number}` is where a MILP search begins (whole or partial; HiGHS completes the rest; part of the memo key, since it can decide a tie or what a limit stops at). `job=True` hands it to a solve job even when it would finish inline (below); it never changes the answer |
| `opt.sum(list)`, `opt.dot(coeffs, exprs)` | Sums, as one expression: Starlark has no `sum()` |
| `sol.status`, `sol.ok`, `sol.feasible` | `"optimal"`, `"infeasible"`, `"unbounded"`, `"infeasible_or_unbounded"`, `"time_limit"`, `"node_limit"`, `"iteration_limit"`, `"interrupted"`, `"pending"` (a solve job has it, below) or `"error"`; `ok` is `status == "optimal"`; `feasible` is "there is a point", which a limit usually leaves and a pending solve does not |
| `sol.objective`, `sol.value(var_or_expr)` | At the solution's point; an integer variable's value is an int. `value()` with no point raises, naming why -- for an infeasible problem, the rules HiGHS found in conflict and their lines, or, for a MILP whose LP relaxation holds, "call sol.conflict() for the rules that clash" |
| `sol.slack(name)`, `sol.binding()` | How far a rule is from its nearest bound (0 within the solver's tolerance); the names of the rules with none, in order. A rule the solve left out (`relax=`) is measured against its own bounds and is negative by as much as the point breaks it |
| `sol.dual(name)`, `sol.reduced_cost(var)` | For an LP: how fast the objective changes as the rule's (or the variable's) binding bound moves up. `None` for a MILP: see below for its local duals |
| `sol.conflict(*, max_probes=None, time_limit=None, families_only=False)` | For an infeasible problem: an irreducible set that cannot all hold -- rule names, then variable bounds as `"x.min"`/`"x.max"`. For an LP, and a MILP whose LP relaxation is infeasible too, HiGHS's IIS, found with the solve (the arguments do not apply). For a MILP whose relaxation is feasible, a deletion filter over the rules, run only when called: whole `family` tags first, the untagged rules as one family more, then halves of the families kept. `max_probes=` and `time_limit=` (seconds, the whole search) bound it, as the evaluation's budget does; stopped early, the rules returned cannot all hold but may not all be needed, and an info says so. `families_only=True` stops after the families. Memoised unless the clock stopped it |
| `sol.gap`, `sol.bound`, `sol.nodes` | A MILP's relative gap, its proven bound on the objective (no point is better) and branch-and-bound nodes; `None`, `None` and 0 for an LP |

**Expressions are linear, and Starlark limits how they are written.**
Variables combine with `+`, `-`, unary `-`, and `*` and `/` by numbers;
`x * y` is an error ("not linear"). starlark-rust has `radd` and `rmul` but
**no reflected subtraction**, so `3 + x` and `3 * x` work but `2 - x` is
Starlark's own "unsupported operand" error: write `-x + 2`, or move the
number into the bound. **Comparisons cannot build rules**: `x <= 3` only
ever gives a bool, so comparing an expression is an error that says to write
`p.constrain(x, max=3)`, and `p.constrain(x == 3)` is refused for the same
reason (`x == y` between two variables is identity, so a variable can be a
dict key). Long sums go through `opt.sum`/`opt.dot`: `a + b + c + ...`
copies the expression at every `+`.

**Working with larger problems**, as `@cadpole/plan` (§B10) does:

- **Options are rules added between solves.** `count=`/`distinct=` only finds
  different *designs* over binaries that decide the design, and the binaries
  of an either/or rule often do not: two rooms can be "apart" on two sides at
  once, so one can flip with nothing moving. `p.constrain()` works after a solve and the
  next solve takes the problem as it then stands, so a library writes its own
  cut -- "some room moves 3 ft past another" -- and solves again.
- **Limits are the normal end of a big MILP**, and they return a result, not
  an error: the best point, its `gap` and `bound`, `feasible` True, and a
  status naming the limit. A good point often comes early and the proof that
  it is the best takes far longer.
  `max_nodes=` counts work and is deterministic, so its answer is memoised and
  repeats; `time_limit=` does neither.
- **A MILP's conflict is found by deletion, when asked.** HiGHS's IIS is an
  LP's, and it is empty for a MILP whose LP relaxation is feasible. For such
  a MILP `conflict()` runs a deletion filter: each probe a solve for any feasible
  point with some rules left out, under the solve's own `time_limit=`, `gap=`
  and `max_nodes=`. Whole families (the `family` tag) go first, staying out
  while the rest is still infeasible, and the untagged rules go together as one
  family more, so a model's thousands of untagged rules cost one probe rather
  than thousands. Within each family that had to stay it then tries halves,
  their halves and so on down to single rules: k needed rules among m take
  about 2k log2(m/k) probes, not m (two among 5,000 untagged ones take 29).
  What is left is irreducible, and where there is a choice it names the rules
  written last.
  - **It is bounded.** `conflict(max_probes=, time_limit=)` -- the time for the
    whole search -- and the evaluation's own budget are checked before every
    probe, and each probe gets only what is left, so none outlives it. Stopped
    early, or with a probe that a limit stopped before it answered (its rules
    stay in), it returns the rules still in: they cannot all hold, and an info
    says the set may not be irreducible. `families_only=True` stops after the
    families -- every rule of the families that cannot go -- which on a big
    plan is the cheap first question. It is memoised unless the clock stopped
    it; a stop at `max_probes=` repeats, so it is kept.
  - **Nothing runs it implicitly.** `solve()`'s info and `value()`'s error use
    only the conflict HiGHS found with the solve, and for a MILP whose
    relaxation holds they say "call sol.conflict() for the rules that clash".
  - **Its probes never start a job**: each reads and fills the
    solution cache like any solve, so a repeated search is all cache hits,
    and a probe that would need a job comes back pending and counts as
    unanswered (its rules stay in, and the set may not be irreducible). The
    search itself is not a job: it runs inside the evaluation and its
    budget.

  `solve(relax=[...], drop=..., objective=False)` is the same probe, for a
  library that wants its own filter.
- **Rules carry tags**, and `@cadpole/plan` writes them in its names:
  `family:floor:room:what` gives a rule `family`, `floor` and `room` tags
  unless `tags=` says otherwise (an explicit `tags=` replaces them; `tags={}`
  is none). `p.constraints(tags=)` lists what a selector picks and
  `solve(drop=)` leaves it out for one solve.
- **A MILP's duals are local, if you want them at all.** `dual()` is `None`
  for a MILP. Fix every integer at its value -- `p.solve(fix={b: sol.value(b)
  for b in binaries})` -- and what is left is an LP, whose duals are the ones
  local to that arrangement: a rule's worth if nothing else moves. Such a
  dual can be several times what a re-solve with the rule moved gives, and a
  fixed arrangement is often degenerate, where several duals are equally
  valid. A rule's real worth to a design is the re-solve with its limit moved. (An
  integer fixed at a whole number -- by `fix=`, or by `min == max` -- is
  solved as a constant, which is what makes a MILP with every integer fixed
  an LP: `nodes` 0, `gap` and `bound` None, `dual()` answered.)
- **Diagnostics.** An infeasible or unbounded answer is an info naming the
  conflict HiGHS found with the solve (or, for a MILP whose relaxation holds,
  pointing at `sol.conflict()`); so is a limit that left a point ("stopped at
  the node limit with a point within 2.5 % of the best bound"), since that is
  the normal end of a big MILP. A limit with no point, or a solver failure, is
  a warning.

**Numbers are plain numbers.** Nothing in `opt` knows about units: a script
that models in feet divides library values by `ft(1)` on the way in and
draws with `ft()` on the way out. It should model in units near 1: HiGHS's
tolerances are absolute (1e-7 on a rule, 1e-6 on integrality), and in
millimetres a room's area is about 1e7 next to a 762-mm door -- big-M rules
in such units are where solvers go wrong (`@cadpole/plan` models in feet,
§B10).

**Determinism, and the one thing it does not cover.** The same problem and
options give the same answer, bit for bit, on every run on one machine, from
`cadpole eval`, `cadpole serve` and `cadpole mcp` alike: one thread, no output, a
fixed random seed. When several points tie for the optimum, which one comes
back is decided by the solver's pivoting, not by the problem: the same on
every run, but the Linux and macOS builds (different compilers, different
libm) may pick different ones. Assert objectives, slacks and the rules a
design must meet -- as `examples/room_layout_opt.star`'s tests do -- not the
coordinates of a tie; break ties you care about with a small term in the
objective. A `time_limit=` that stops a solve makes its answer depend on the
clock: its status says `"time_limit"`, the point is the best found, and it is
never memoised. A solve is also bounded by the evaluation's own budget (20 s
in `cadpole serve`, 120 s in the CLI or what `--eval-budget-secs` sets), passed to HiGHS as its time limit,
because the deadline cannot interrupt a native call; where the solution
cache is on (below), a solve the budget would stop gets a window of it
instead and, past the window, comes back `"pending"`, not `"time_limit"`.

**Long solves run as jobs**. Every native evaluation reads a
persistent solution cache before it solves, and files there every answer
that does not depend on the clock, so the same question is solved once per
machine, not once per process (`~/.cache/cadpole/opt`; `CADPOLE_OPT_CACHE` moves
it and `CADPOLE_OPT_CACHE=off` turns the cache and jobs off;
`CADPOLE_OPT_CACHE_MB` caps it, 512 MB by default). A solve gets a window of
what the evaluation has left -- half of it and at most 10 s in `cadpole serve`
and `cadpole solve`, all but a tenth where no job can start -- and one that
needs longer, or is written `solve(job=True)`, or asks for a `time_limit=`
past the window, comes back with status `"pending"`: `ok` and `feasible`
are False, `objective`, `gap` and `bound` are None, `value()`, `slack()`
and `binding()` raise naming the job (`job 3f9a2c1b7e44`), and an info says
the same at the `solve()` line. Where the host runs jobs -- `cadpole serve`
for its owner's evaluations, and `cadpole solve` -- the solve is then running
in a process of its own (`cadpole opt-job`, under a time and a memory limit),
and the document is evaluated again when it lands, with the answer;
elsewhere the message says what runs it. Only an evaluation its owner
asked for may start a job (`Limits::solve_jobs`, which a host sets): a drag
preview, the limited listener, a render, a sweep, an export, a test run and
a plain `cadpole eval` read cached answers and attach to running jobs, but
start none. `job=` never changes the answer, only where it is computed, so
it is not part of the memo key. A script that chains solves (floor by
floor) should test `sol.ok` before reading values: each evaluation then
gets one link further, and each landing starts the next. A job stopped by
the `time_limit=` its solve asked for keeps that limited answer (status
`"time_limit"`, with an info "kept from job ..."); one stopped by the
host's own job limit is reported, not kept.

**Running them.** `cadpole solve doc.star` evaluates, runs every pending
solve as a job (`--jobs N` at once, each under `--time-limit S` and
`--mem-mb MB`), waits, and evaluates again until nothing is pending
(`--detach` starts them and returns; `--rerun` runs again the solves whose
last job failed, was cancelled or hit a host's limit); `cadpole eval --jobs
wait` does the same before its own output. `cadpole jobs` lists the machine's
jobs, whoever started them, `cadpole jobs wait` waits for them and `cadpole jobs
cancel ID` stops one (`--forget` lets the next evaluation solve it again).
`cadpole serve` runs its owner's jobs by itself (`--solve-jobs`,
`--job-time-limit`, `--job-mem-mb`, `--solve-inline-ms`, `--no-solve-jobs`)
and emits `solve_started` and `solve_finished` on its event stream; over
MCP the same is `solve`, `list_jobs`, `wait_jobs` and `cancel_jobs`. A
one-shot command says on stderr what its solves did ("opt: 1 answer from
the cache, 0 solves here ...; 1 solve pending: `cadpole solve` runs it").

**Memoised**: a solve is keyed by the SHA-256 of everything in the
problem -- names, bounds and coefficients in declaration order, which decides
ties -- its options and the solver's version, so re-evaluating an unchanged
script in `cadpole serve` does not solve again. `solve()` never raises for an
infeasible or unbounded problem, so a test can assert one; an info names the
conflict, and a limit with no point, or a solver failure, is a warning. A
conflict search is memoised under the solve's own key, with `max_probes=` and
`families_only=`.

Caps: 250,000 variables, 500,000 rules and 2,500,000 coefficients a problem;
1,000 solutions a `count=`. `examples/room_layout_opt.star` lays out four
rooms with minimum sizes, a bound on how deep two may be for their width,
non-overlap, a shared wall and
`@cadpole/building.star`'s `floor_area_cap()` as a rule; `stdlib/opt_test.star`
is the API as `cadpole test` runs it.

---

## B10. Floor plans: `@cadpole/plan`

`@cadpole/plan` is a family of standard-library modules (§4.14) that hold a
floor plan, check it, furnish it, frame it, solve for it and draw it, most
of them on `opt` (§B9). They share one value, **the plan**: plain data in
feet that any module -- or a script, an adapter for a house drawn some other
way, a solver -- can write, and that every other module reads. The fixtures
and examples are synthetic.

| Modules | What they are |
| --- | --- |
| `@cadpole/plan.star` | the plan value: constructors, `validate()`, the shared queries, a room in its own frame, feet-inches |
| `@cadpole/plan_draw.star` | a plan on sheets: floors in layers, labels clear of each other, the red pen, plan sets, a comparison sheet |
| `@cadpole/plan_audit.star`, `@cadpole/plan_light.star` | the red pen: code, doors, stacks, fillers, daylight, furniture and isovists as check rows and marks |
| `@cadpole/plan_furniture.star`, `@cadpole/plan_furnish.star`, `@cadpole/plan_fit_tables.star`, `@cadpole/plan_fit_tables_gen.star`, `@cadpole/plan_kitchen.star`, `@cadpole/plan_roof.star` | the room stage: furniture packed in a room, fit tables and the fit row, a kitchen run, roof PV rows |
| `@cadpole/plan_solve.star`, `@cadpole/plan_ask.star` | floor plans as MILPs of free rectangles, and questions about a plan of your own |
| `@cadpole/plan_frame.star`, `@cadpole/plan_spans.star` | a fixed plan's cheapest frame, and the span tables |
| `@cadpole/plan_parti.star`, `@cadpole/plan_templates.star`, `@cadpole/plan_rules.star` | parti first: a template fitted on every floor by one MILP, the four 25-ft partis, the rule menu |

What every module keeps:

- **Feet.** Every length in a plan, and every length a module takes or
  returns, is feet (a float); areas are square feet. A module that makes
  geometry converts with `@cadpole/building.star`'s `ft()` at the last moment;
  the room stage works in inches inside and speaks feet. Text is
  feet-inches, and a clear width is written rounded down.
- **Loading records nothing** (§4.14). A function that solves --
  `furnish()`, `solve()`, `reframe()`, `fit()`, `explain()`,
  `kitchen_run()`, `pv_rows()` and everything that calls them -- makes
  `opt.problem()` features, and the light measures make `view_cone` boxes
  on a hidden layer, `plan_light`; all take `name=`, so a document that asks
  the same question twice passes two names.
- **Check rows** have one shape, `{key, name, ok, detail, rule, severity,
  floor, room, at | rect}`, with `severity` `"fail"`, `"warn"` or `"note"`.
  `validate_rows()`, the audit, `frame_checks()`, `precheck()` and
  `check_rules()` write them, and `plan_draw` numbers the failing ones on
  the sheets: a red-pen ring where a row has an `at` or a `rect`, a list
  entry where it has neither.
- **Rule names** in every MILP are `<family>:<floor>:<room>:<what>`, which
  `opt` reads as tags (§B9), so `p.constraints(tags=)`, `solve(drop=)`
  and `sol.conflict()` work by family and floor.
- **Long solves are jobs.** A fit too long for one evaluation comes back
  pending and runs as a job where the host has them (§B9):
  `cadpole solve` in the CLI, and `cadpole serve` by itself.

```python
load("@cadpole/plan.star", "toy_house", "validate")
load("@cadpole/plan_draw.star", "plan_set")
load("@cadpole/plan_audit.star", "audit", "row_line")

H = toy_house()                          # a synthetic two-floor house, 25 x 60 ft on a 25 x 100-ft lot
assert_eq(validate(H), [])               # nothing wrong with it as data
A = audit(H, name = "toy_audit")         # the red pen: {"rows", "marks", "count"}
for r in A["rows"]:
    if not r["ok"]:
        print(row_line(r))               # "FAIL key: name -- detail", or WARN, NOTE
S = plan_set(H, name = "toy", checks = A["marks"])
export(S, "toy.pdf")                     # the floors on a sheet set, the marks numbered
```

`examples/plan_toy_house.star` draws the toy house and a variant as a plan
set with a comparison sheet; `examples/plan_parti_compare.star` compares
four partis fitted on three floors.

#### The plan value: `@cadpole/plan.star`

A plan is dicts, lists, tuples, numbers and strings, nothing else. Any
module can build one by hand; `normalize()` fills in what a hand-built plan
leaves out, and `validate()` says what is wrong with one however it was
built.

**Frame and units.** Every length is in feet (a float), every area in
square feet. u runs east, v north. A rect is `(u0, u1, v0, v1)` at the
inside faces, u0 < u1 and v0 < v1. A wall is named by the side of the room
it bounds: `"N"`, `"S"`, `"E"`, `"W"`. Along a wall, positions run west to
east on an N or S wall and south to north on an E or W wall; `"lo"` is the
lower-coordinate end.

**Tolerances** (module constants):

| Name | Value | Meaning |
| --- | --- | --- |
| `WALL_GAP` | 1'-0" | two rooms share a wall when their facing inside faces are 0 to 1'-0" apart: partitions run 4 to 7 1/4 in, and the rest of the foot takes 8- to 12-in concrete, party and fire walls. Every query that looks across a wall takes `gap=` |
| `TOL` | 5/8 in (0.05 ft) | a face may run this far into its neighbour, and a door or window this far past its wall's end, and still count; rooms overlap when they run into each other more than this both ways |
| `STACK_REACH` | 7 1/4 in (0.6 ft) | a stack belongs to a room within this of it (in its wall) |
| `COURT_REACH` | 2'-6" | an exterior wall faces a court, well or slot whose edge is within this of the room's inside face |
| `DOOR_WIDTH` | 2'-8" | a door's default clear width |

Exterior walls are found against the floor's `plate` when it has one (a
side is exterior where the point `WALL_GAP` beyond it is outside the
plate), else derived (no other room within `WALL_GAP` beyond it); outdoor
rooms never cover a wall, and pieces shorter than `WALL_GAP`, where
partitions end, are dropped.

**The plan** (defaults in brackets):

```python
{"name", "unit": "ft", "source": "", "site": {...}, "floors": [...], "rooms": [...], "doors": [...], "windows": [...],
 "fixtures": [...], "furniture": [...], "stacks": [...], "courts": [...], "walls": [...], "bearing": [...], "meta": {}}
```

| Record | Fields |
| --- | --- |
| site | `lot` rect and/or `lot_poly` [(u, v)...]; `street` ["S"]; `north_deg` [0.0] (true north, degrees clockwise from +v); `setbacks` {front, rear, east, west}; `neighbors` [{rect, height}] |
| floor | `key`; `name` [key]; `z` [0.0]; `ceiling` [8.0]; `plate` (a list of rects: the built footprint at the inside faces of its exterior walls) |
| room | `key`; `name` [key]; `floor`; `kind`; `rects` (main first; an L's second touches it, or faces it across one wall); `unit` ["main"]; `habitable`, `sleeping` [by kind]; `kept` [False]; `ceiling` [the floor's]; `core` (one key for a stair or lift on every floor, or for a flight and the room it lands in); `furnish` (the furniture set it holds). Other keys are kept (an adapter's `note`, a stair's `flight_width`) |
| door | `key`; `floor` [room a's]; `a`; `b` (a room, or `"outside"`, `"outside:street"`, ...); `wall` (room a's side); `at` (the opening's centre on the wall line); `width` [2'-8"]; `swing` ["a"]: "a", "b", "slide", "pocket", "open"; `hinge` ["lo"]; `span` (lo, hi): where a solver may put it before `at` |
| window | `key`; `floor` [the room's]; `room`; `wall` (None for a skylight); `at`; `width` [3'-0"]; `sill` [2'-0"], `head` [7'-0"] over the room's floor; `source` ["side"]: "street", "rear", "side", "court:<key>", "well:<key>", "slot:<key>", "deck", "roof"; `egress` [False]; `clear` (w, h), the net clear opening. A skylight may carry its own `rect` |
| fixture | `key`; `room`; `kind` (free text: "wc", "lav", "tub", "shower", "sink", "range", "fridge", "island", "washer_dryer", ...); `rect`; `front` ["N"] |
| furniture | `room`; `kind`; `rect`; `front` (the room stage's output) |
| stack | `key`; `at`; `floors`; `kind` ["dwv"]; an adapter marks a drain the model does not draw `implied=True` |
| court | `key`; `kind` ("court", "well", "slot"); `rect`; `base` (its bottom floor); `top` (where open sky starts); `reaches` ("street", "rear", None: whether it opens to a yard that reaches the public way) |
| wall | `floor`; `a`, `b` (centreline ends); `t` [0.5]; `kind` ["partition"]: "exterior", "partition", "bearing", "concrete", "party", "fire". With none, walls are derived from the rooms |
| bearing | `floor` (the floor whose walls carry it); `axis` ("v" runs north-south at u = `at`; "u" east-west at v = `at`); `at`; `span` |

**Room kinds** (`KINDS`, grouped by `KIND_GROUPS` in `GROUPS` order):
living -- living dining family den study office play media guest bed; wet
and service -- kitchen bath powder wc laundry pantry; circulation -- hall
entry foyer mud stair lift landing lobby; storage -- closet storage mech
garage workshop; outdoor -- deck court well; other. `habitable` defaults to
True for the living kinds and the kitchen, `sleeping` for bed and guest.
`PLUMBED` (kitchen bath powder wc laundry) are the kinds that sit on a
stack; `SWINGS`, `SOURCES`, `COURT_KINDS` and `WALL_KINDS` are the other
vocabularies.

**A room in its own frame**, `room_local(p, key)`, is what the room stage
packs: the origin is the main rect's south-west inside corner.

```python
{"key", "name", "floor", "kind", "furnish", "origin": (u0, v0), "w", "d", "ceiling", "rects": [local, main first],
 "doors":   [{"key", "wall", "offset", "width", "swing": "in" | "out" | "slide" | "pocket" | "open", "hinge", "to",
              "lo", "hi", "rect", "point"}],
 "windows": [{"key", "wall", "offset", "width", "sill", "head", "egress", "source", "lo", "hi", "rect", "point"}],
 "fixtures": [{"key", "kind", "rect": local, "front"}],
 "stacks":  [(u, v) local, within STACK_REACH],
 "furniture": [{"kind", "rect": local, "front"}]}
```

`offset` is the opening's **centre** along its wall from the wall's start:
the opening spans `offset - width/2` to `offset + width/2`, which `lo` and
`hi` also give. `wall` is this room's side (for room b of a door, the side
facing room a); `rect` is which of the room's rects the wall is on, and the
offset is from that rect's side; `point` is the opening's centre, local.
`"in"` means the leaf swings into this room. Skylights are left out (they
are in no wall).

| Function | What it does |
| --- | --- |
| `site(lot=None, *, lot_poly, street, north_deg, setbacks, neighbors)`, `floor(key, *, name, z, ceiling, plate)`, `room(key, name=None, *, floor, kind, rects \| rect, unit, habitable, sleeping, kept, ceiling, core, furnish, **extra)`, `door(key, *, a, b, wall, at, floor, width, swing, hinge, span, **extra)`, `window(key, *, room, wall, at, floor, width, sill, head, source, egress, clear, **extra)`, `fixture(key, *, room, kind, rect, front, **extra)`, `stack(key, *, at, floors, kind, **extra)`, `court(key, *, kind, rect, base, top, reaches, **extra)`, `wall(*, floor, a, b, t, kind)`, `bearing(*, floor, axis, at, span)` | Constructors: each fills the defaults and fails with the first of `check_record`'s messages, naming the record |
| `plan(name, *, floors, rooms, doors, windows, fixtures, furniture, stacks, courts, walls, bearing, site, source, meta)` | A plan, normalised (doors and windows take their room's floor) |
| `check_record(what, rec)`, `rect_problem(r, what)` | One record's problems alone, as messages |
| `normalize(p)` | The defaults filled into a plan built by hand |
| `validate(p)`, `validate_rows(p)` | Every problem, as sentences, or as check rows (rule `"plan"`): malformed records (a rect in the wrong order, an unknown kind or swing, a head under its sill); keys used twice; dangling keys; rooms on one floor overlapping beyond `TOL`; a room off its floor's plate; a room whose rects do not reach its main rect; a core twice on a floor, or a stair on one floor and a lift on another; a door not on a wall its rooms share (or, to the outside, not on an exterior wall of room a); a window not on an exterior wall of its room, or not facing the court its source names; a fixture outside its room |
| `get_room`, `get_floor`, `get_door`, `floors_by_z(p)`, `rooms_on(p, floor, *, kinds, unit)`, `doors_of`, `windows_of`, `fixtures_of`, `stacks_on`, `courts_on` | Lookups; `kinds` is a list or a group name |
| `kind_group`, `default_habitable`, `default_sleeping`, `is_habitable`, `is_sleeping`, `is_outside`, `is_source` | The vocabularies, asked |
| `area(x, *, floor, kinds, unit)`, `width`, `depth`, `dims`, `bbox(x, *, floor)`, `centre(x)`, `clear_width(x)` | Measures: of a rect, a union of rects, a room (the main rect's width and depth) or a plan's rooms; `clear_width` is a room's narrowest clear dimension (a hall's or landing's width) |
| `shared_walls(p, a, b, *, gap, min_len)`, `neighbours(p, key, *, gap, min_len)` (also `neighbors`) | Walls two rooms share (wall, line, far, gap, span, length, a_rect, b_rect), pieces of one line that meet end to end merged |
| `exterior_walls(p, key, *, gap, min_len, mode)` | A room's exterior walls (room, rect, wall, line, span, length), each with `faces` -- what it looks onto: "street", "rear" or "side" by the street's side, "court:<k>", "well:<k>", "slot:<k>", "deck:<k>" -- and `facade`, "street", "rear" or "side" by direction alone, for fire separation |
| `door_graph(p, *, floor, cores)`, `door_path(p, a, b, *, avoid_kinds, avoid, units, through_outside, cores, edges)`, `walk_length(p, a, b, ...)` | The door graph ({room: [{to, door, via}]}, with `"outside"` and the cores' links between consecutive floors); the way of fewest doors, avoiding kinds or rooms, and with `units` staying inside those dwelling units (None when there is none); that way's straight runs, centre to door to door to centre, a lower bound on the walk |
| `rooms_crossed(p, floor, axis, at, *, span)` | The rooms a bearing line or beam passes through, and how far |
| `room_local(p, key, *, gap)` | A room in its own frame (above) |
| `plan_diff(a, b, *, tol)` | Rooms added, removed and changed -- moved, resized, on another floor or of another kind -- largest move first: what a solver's next option really changed |
| `door_between(p, key, a, b, *, wall, along, width, swing, hinge, gap)`, `window_on(p, key, room, wall, *, along, width, sill, head, source, egress, clear, gap)` | A door on two rooms' longest shared wall (or `along` it; to the outside, on room a's exterior `wall`), and a window whose source is what the wall faces |
| `ftin(x, frac=None, down=False)`, `sqft(x, digits=0)`, `size_text(x, frac=None)` | 2'-6"; `frac=16`: 2'-11 15/16"; `down=True` rounds toward zero, so a clear width of 2'-11 15/16" writes 2'-11", never the 3'-0" it misses. 1,234 sq ft. 13'-6" x 21'-7" |
| `toy_house()` | A synthetic two-floor house, 25 x 60 ft on a 25 x 100-ft lot, with a stair core, an L-shaped kitchen, a powder room, a bath with fixtures, stacks, a roof deck, windows with sources and egress, and doors to the street and the yard; any module can test on it |

The purity probe is `ftin`. `stdlib/plan_test.star` holds the tests.

#### Drawing: `@cadpole/plan_draw.star`

A plan value on the sheet API (§6.4), laid out first and painted after,
so that a layout can be tested and painted on any sheet.

| Function | What it does |
| --- | --- |
| `floor_layout(p, floor, *, scale=6.35, checks, numbers, dims, labels, fixtures, furniture, details, highlight, style)` | One floor laid out in paper millimetres (6.35 mm a foot is 1/4" = 1'-0") without a sheet. Returns `w`, `h`, `s`, `origin`, `margin`, `bbox`, `shapes`, `texts`, `labels` (room key: how it was placed -- `"inside"`, `"shrunk"`, `"upright"` or `"outside"` -- its box, rect and lines), `overlaps` (labels that found no free place), `marks` and `rows`. `highlight` outlines rooms in the accent colour |
| `paint(sheet, lay, at)`, `draw_floor(sheet, p, floor, *, at, scale, checks, numbers, title, number, dims, labels, details, style)` | A layout on a sheet; both at once |
| `plan_sheets(S, p, *, checks, floors, prefix="A", first=1, per_sheet, scale, title, style)` | The floors on sheets at the largest architect's scale that fits, up to 1/2" = 1'-0", with the kinds' key and the red pen's list in a column |
| `plan_set(p, *, name, checks, size, project, stamp, per_sheet, scale)` | A sheet set with a title block |
| `compare_sheet(S, plans, *, number="C1.0", name="COMPARISON", scorecards, checks, labels, floors, style)` | N plans side by side at one scale, each plan's floors in a row or a stack, on the panel grid that gives the largest scale, with a scorecard table of a row per plan; in every plan after the first, the rooms `plan_diff()` says it added or changed are outlined |
| `scorecard(p, checks)` | Rooms, net sq ft, bedrooms, plan problems, failing checks |
| `redpen(checks, *, notes=False)` | `[(n, row)]` for the failing rows, numbered as the sheets number them |
| `scale_for(...)`, `kind_fill(kind)`, `STYLE`, `KIND_FILL` | The scale chooser, the fill by kind, the style |

A floor is drawn in layers: courts open at the floor and decks; the walls
-- the plan's own on the floor, else the plate filled as wall and every
indoor rect grown by 6 in, so the gaps between rooms read as partitions;
rooms filled by kind, stairs hatched across their run, lifts crossed; door
openings cut through the wall, with leaves and quarter-circle swings
(`"slide"` two panels, `"pocket"` a dashed leaf in the wall); windows across
the exterior wall (escape glass in blue, skylights dashed at their `rect`);
fixtures with symbols, furniture, stacks (a blue dot on a white ring) and
bearing lines; overall dimensions; one label per room -- name, sq ft, and
the clear W x D rounded down to the inch -- tried at the centre of each rect
and around it, clear of every other label, the red pen's marks, fixtures and
skylights, shrunk to 85 % and 72 %, cut to fewer lines, turned upright in a
narrow room, or set in a column outside with a leader; and last the red
pen. `cadpole export x.star --run-exports --out-dir DIR` writes the PDF. The
purity probe is `kind_fill`; `stdlib/plan_draw_test.star` holds the tests.

#### The red pen: `@cadpole/plan_audit.star` and `@cadpole/plan_light.star`

An audit of a fixed plan -- a solver's, or one drawn by hand and read
through an adapter -- that marks what is wrong with it. It checks; it does
not design. Its code readings are unverified and its light thresholds
uncalibrated, and every row that depends on either says so.

| Function (`@cadpole/plan_audit.star`) | Returns |
| --- | --- |
| `audit(plan, program=None, rules=None, furnish=True, light=True, validate=True, name=None, isovists=True)` | `{"rows", "marks", "count"}`: every family's rows, `redpen(rows, plan)`, and the counts by fail, warn, note and pass |
| `code_rows(plan, rules=None)` | R304, R305, R310.1 (and basements), R311.6, R311.7 (width, landings), CPC 402.5 with R307.1 beside it, showers, R302.5.1, R302.1 |
| `door_rows(plan, rules=None)` | The door graph's rows: reach, the garage, baths, powder rooms, bedrooms, the pantry, the kitchen's doors, halls, walks |
| `stack_rows(plan, rules=None)` | Plumbing stacks |
| `filler_rows(plan, program=None, rules=None)` | Fillers, storage and hall shares, windows, shape |
| `furnish_rows(plan, rules=None, rooms=None, name=None)` | The room stage at the drawn sizes: `furnish(room_local(plan, key))`; a set that does not fit is a fail with the depth it needs |
| `isovist_rows(plan, rules=None)`, `isovist(plan, floor, at, rays=180, doors="all")` | Openness from the entry and the stair, reported only |
| `redpen(rows, plan=None, kinds=("fail", "warn", "ask"))` | The marks: check rows `plan_draw` draws (below) |
| `redpen_set(plan, rows, name=None, project=None, stamp=None, per_sheet=None, scale=None)` | `plan_draw`'s `plan_set` with the marks, exported as `<name>.pdf` |
| `door_graph(plan)`, `reach(G, starts, banned)`, `entries(plan, G, unit)`, `walks(plan, floor, start_room, start_pt, targets, banned)` | The graph and its walks, Manhattan through door points |
| `fire_distance(plan, floor, p, side)`, `stair_width(room)` | R302.1's distance to the lot line; a stair's flight width (`flight_width`, `stair_width` or `stair["width"]`, else the main rect's least side) |
| `row_line(row, prefix="")`, `failures(rows)`, `questions(rows)` | `"PASS key: name -- detail"` (or FAIL, WARN, NOTE); the failing rows; the questions |
| `ftin(x, frac=1)`, `inches(x, frac=16)`, `sqft(a)`, `pct(x)` | Feet-inches, inches to the sixteenth, sq ft, % |
| `RULES`, `FAMILIES` | The thresholds `rules=` overrides; the family names |

`rules` is a dict of `RULES` keys (and `@cadpole/plan_light.star`'s `LIGHT`
keys) to override, plus `"skip"`, a list of row keys or key prefixes to
leave out (`"code:R305"`, `"light"`), and `"furnish_rooms"`, the rooms to
re-pack. `program` (for the filler rows) is a list of room keys or kinds,
or of dicts with `"key"` or `"kind"` (and `"count"`), or a dict of key or
kind to a count or a spec; circulation, outdoor and kept rooms are never
"unasked".

**Rows and marks.** A row's `key` is `<family>:<check>:<floor>:<room>`, so
`rules["skip"]` and a reader can filter by prefix. `severity` is the rule's
weight: `"fail"` is code, or a plan that does not work (no way from the
garage into the house); `"warn"` a house rule or an architect's mark (the
pantry's walk, a 2:1 bedroom); `"note"` a measure, an unknown, or a
question. `ok` is whether it held; a row that could not be decided is `ok`
False at severity `"note"` -- an escape window with no net clear opening
given ("unknown until the net clear opening is drawn"), or a question for
the building department (an `"ask"` mark below). `detail`
starts with the finding, short enough for a mark ("Bath: its only door is
from the garage"), then `"; "` and the rest. `redpen(rows, plan)` returns
the rows that did not hold as display copies, ordered floor by floor so
`plan_draw` numbers them the way a marked-up sheet reads: each a check row
whose `name` is the short finding and `detail` the rest, plus `n`, `text`,
`kind` (`"fail"`, `"warn"`, `"ask"`) and `of` (the row's key). A row with a
place (`at`, `rect`, or a room of the plan) gets a ring; one without (a
floor's shares) is numbered and listed. A question's copy is a `"warn"`,
because `plan_draw` leaves notes out. Draw them with
`plan_set(p, checks=redpen(rows, p))`, or `redpen_set(p, rows)`. One
complaint, one mark: a room that R304's width or the re-pack marks keeps
its 2:1 row as a note, and a windowless room keeps its dark light row as a
note. Kept rooms' dimensional rows are notes ("an existing room, kept").

| Function (`@cadpole/plan_light.star`) | Returns |
| --- | --- |
| `lit_depth(head=8.0)` | 2 x head: how deep facade glass lights a room (uncalibrated) |
| `glass_doors(plan, head=8.0)` | The plan's sliding doors to the outside or onto a deck, as window records: glass, for the light family |
| `lit_share(plan, floor, court_lit=None, depth=None, step=1.0, rules=None)` | `{"total", "lit", "share", "rooms": {key: (sq ft, lit sq ft)}}` on a 1-ft grid: each habitable room against its own glass and that of rooms open to it through a cased opening; `rules={"mode": "level"}` counts any glass on the floor, through walls (with `depth=15`, a level-wide 15-ft measure) |
| `street_sky(depths=(2.0,), floor_z=None, head=None, rays=None, name=, **geom)` | The reference street window's sky at each depth, `{depth: fraction}` (makes boxes) |
| `court_sky(w, top, floor_z=0.0, depths=(2.0,), head=8.0, span=None, length=20.0, along=None, front=12.0, rear=12.0, back=17.0, opp=None, opp_top=None, wall=None, rays=None, name=)` | A court window's sky on parametric geometry (makes boxes) |
| `street_reference(head=8.0, rays=None, name=)` | `{"near", "far", "head"}`: the street window's sky 2 ft in and at 2H |
| `massing(plan, name=, rules=None)`, `window_sky(plan, w, M=None, depths=(2.0,), rules=None, name=)` | A plan as `view_cone` boxes, and one window's sky on it (make boxes) |
| `sky_table(widths, heights, floors, storey=10.0, head=8.0, length=20.0, rays=None, name=)` | The generator of `SKY_TABLE` (makes boxes) |
| `SKY_TABLE`, `sky_row(w, h, floor=0, table=None)`, `lit_pieces(h, floor=0, table=None)`, `sky_table_text(T)` | The committed table (court width x wall height above the head x floor); its lookup, bilinear in width and height; the (width, lit) breakpoints for a solver; the table as source |
| `light_rows(plan, rules=None, measure=True, name=None)` | The light family's rows; `measure=False` estimates from `SKY_TABLE` and makes nothing |
| `LIGHT`, `STREET` | The light thresholds, and the reference street window's geometry |

Facade glass (street, rear, side, deck) lights the floor within twice its
head; a court, well or slot window gets `view_cone` (§B8): the share of a
180-degree by 0-to-89-degree band of sky that an eye 2 ft inside and 2'-6"
up sees past the plan's massing and the site's neighbours, 64 x 64 rays,
reported against the reference street window (0.456), and its lit depth,
how far in the eye can go before it sees less sky than an eye 2H behind
street glass. Only the wall above the window's head matters, so a court's
lit depth is a function of its width and that wall. For a plan MILP the
light is one row: `sky_row(w, h, floor)` gives `{"w", "h", "floor", "sky",
"rel", "lit"}`, `lit` the depth in feet a court window lights, so a fixed
court width writes `depth <= lit`, and with the width a variable
`lit_pieces(h, floor)` gives the breakpoints to encode. The light family
makes `view_cone` boxes on a hidden layer, `plan_light`, and the re-pack
makes `opt` problems; both take `name=`. Probes: `ftin` (`plan_audit`),
`lit_depth` (`plan_light`). `stdlib/plan_audit_test.star` and
`stdlib/plan_light_test.star` hold the tests.

#### The room stage: `@cadpole/plan_furniture.star`, `@cadpole/plan_furnish.star`, `@cadpole/plan_fit_tables.star`, `@cadpole/plan_kitchen.star`, `@cadpole/plan_roof.star`

The furniture in one room, solved once the room's size is fixed, and the
same furniture as one row per room in a plan MILP.

| Module | What | Probe |
| --- | --- | --- |
| `@cadpole/plan_furniture.star` | The catalog, in inches: `PIECES` (50: rigid shapes of solid rects, use spaces and use points, heights, wet and fixture flags, sources), `SETS` (33), `SET_FOR_KIND`, `FIT_TABLE_KIND`, `SOURCES`, `TALL`, `set_items()`, `piece_shapes()` | `catalog_probe` |
| `@cadpole/plan_furnish.star` | The room stage: `furnish`, `least_depth`, `least_depth_record`, `holds`, `fit_table`, `fit_lookup`, `fit_row`, `walk_check`, `DOOR_CLASSES`, `OP_BED`, `OP_BATH`, `OP_FRONT`, `OP_HOUSE`, `CORE` | `furnish_probe` |
| `@cadpole/plan_fit_tables.star` | Generated data: `FIT_ENTRIES` (170), `GRID_ENTRIES` (148), `FIT_TABLES` (54 tables) | `fit_tables_probe` |
| `@cadpole/plan_fit_tables_gen.star` | The tables' generator: `fit_jobs`, `grid_jobs`, `GRID`, `solve_jobs`, `record_source`, `table_source`, `clear_w`, `depth_in`, `stretch_test`, `STRETCH` | `fit_jobs` |
| `@cadpole/plan_kitchen.star` | A kitchen run: `kitchen_run`, `check_run`, `NKBA`, `SIZES` | `kitchen_probe` |
| `@cadpole/plan_roof.star` | PV rows: `pv_rows`, `fire_setbacks`, `LOW_SLOPE`, `PATHWAY`, `RIDGE_18`, `RIDGE_36` | `roof_probe` |

**`furnish(local, set_name=None, *, doors=None, open_sides=None,
wet="stack", strict=False, door_step=18.0, jamb=2.0, keep_fixtures=None,
obstacles=(), without=(), rewalk=True, time_limit=30.0, explain=True,
name=None)`** packs a set into a room. `local` is `room_local(plan, key)`'s
record (feet; each opening's `offset` its centre); `set_name` defaults to
`local["furnish"]`, then `SET_FOR_KIND[local["kind"]]`. It returns `{"ok",
"reason", "set", "furniture", "fixtures", "clears", "wet_walls", "cuts",
"status", "layout", "W", "D"}`, and, when not ok, `need_depth` (ft) or
`relaxed` (the fewest-wet-walls layout, when only the stack wall fails).
`furniture` and `fixtures` are plan records `{"key", "room", "kind",
"item", "piece", "rect", "front", "h"}`, rects in plan feet; `clears` the
use spaces. The rules:

- solids stay out of each other's solids and use spaces; use spaces may
  overlap;
- a door swinging in keeps its opening (the leaf plus `jamb`) by the leaf
  plus `door_step` (the 18 in to step past it) free of solids; a door
  swinging out, sliding or in a pocket keeps `door_step` inside it; a cased
  opening, 36 in; `strict=True` keeps use spaces out of the leaf's own
  swing too;
- a 24-in zone in front of each window is kept free of anything taller than
  its sill;
- `wet="stack"`: wet pieces back onto a wall within 0.6 ft of one of
  `local["stacks"]` (with none, the fewest wet walls); `"fewest"`: wet walls
  cost 100 each in the objective; `"any"`: no rule;
- `obstacles=[{"rect": plan rect, "h": ft or None}]` are built-ins (a closet
  and its doors' 30 in, a casement's swing); `without=("closet",)` leaves
  items out;
- the re-walk: every use point must be reachable from some door on a 2-in
  grid, else that placement is cut and the MILP solves again, six times at
  most; `rewalk=False` skips it;
- a room of several rects (an L) is packed in its bounding box, the box's
  parts outside the room kept clear and blocked for the walk; doors in its
  inner walls are kept clear and walked from; pieces back onto the box's
  walls only;
- a set the catalog lacks is a not-ok result naming the sets it has, never
  an error;
- the problem is the feature `name` (default
  `plan_furnish_<room>_<set>_<W>x<D in>`).

**`least_depth(set_name, width, door_class, *, detail=False,
time_limit=60.0, name=None)`** is the least clear depth (ft) that holds a
set at a clear width (ft) with a door class (`DOOR_CLASSES`), every use
point reachable, or None if nothing up to 40 ft does;
`least_depth_record(set_name, width_in, door_class)` is the same in inches
with the layout. **`holds(set_name, width, depth, door_class, *, width_in=,
depth_in=)`** asks whether a room holds the set. **`fit_table(set_name,
door_class, widths)`** makes a table, `{"set", "door", "rows": [(width,
depth or None)]}`. **`walk_check(lay, starts,
targets)`** is the targets a person cannot reach.

**Fit tables.** `FIT_TABLES["<set>|<door class>"]` holds 54 tables of rows
`(clear width ft, least clear depth ft or None)`, sorted by width, each
depth the solved inches rounded up to a millionth of a foot (a fit row may
only err deep). The sets: `full`, `den`, `tub`, `shower`, `powder`,
`laundry`, `pantry`, `living`, `dining`, `kitchen_L`, `kitchen_island`,
`entry`, `car`, `car_storage`, and from the 6-in grid `full` 8'-0" to
16'-0", `twin` 7'-0" to 14'-0" and `queen` 10'-0" to 18'-0" (classes `opp`,
`adj`), `tub` and `shower` 5'-0" to 9'-0" and `powder` 3'-0" to 6'-0"
(classes `wide`, `side`). Door classes describe where a room's door and
glass are (`DOOR_CLASSES` has each): `opp`, `adj`, `core`, `wopp`, `wadj`
for bedrooms and dens (`+garden` adds a 6-ft slider), `wide`, `side`,
`mid` for baths and service rooms, `front:N`, `front:WN`, `rear:N`,
`rear:WN`, `mid:NS`, `mid:WNS` for day rooms by facade and open sides,
`entry:a0`, `entry:a`, `garage`. `FIT_ENTRIES` keeps every job with its
provenance.

**`fit_row(p, w, d, table, *, name=None, w_max=None)`** adds "room depth `d`
is at least the table's least depth at room width `w`" to plan MILP `p` and
returns the rows' names; name it `"fit:<floor>:<room>:depth"`, the family
tag. With `w` a one-hot width, `[(binary, width), ...]` with exactly one
binary 1, it is one row, `d >= sum_k z_k f(W_k)`, exact at every choice (a
width the table cannot hold gets its binary fixed to 0). With `w` a
variable it is the step function `f(w) = D_m` for `W_m <= w < W_(m+1)` in n
monotone binaries `s_1 >= ... >= s_n` ("w has passed W_k"):

    w >= W_0 + sum_k (W_k - W_(k-1)) s_k          (one row)
    d >= D_0 - sum_k (D_(k-1) - D_k) s_k          (one row)
    s_k <= s_(k-1)                                (n - 1 rows)

whose projection is exactly `w >= W_0` and `d >= f(w)`: a room narrower than
`W_0` is refused, and `w_max` drops steps `w` can never reach.

**`kitchen_run(length=None, *, sizes=None, name=None)`**: with no length,
the shortest run that meets every NKBA landing; with a length (ft), the
least total shortfall there. **`check_run(order, counters, *, sizes=None)`**
checks a drawn run (counters `c0` to `c3` in inches, west to east). Both
return `{"ok", "order", "counters" (in), "length" (in), "sides"}`, and with
a length `"short"` `{counter: in}` and `"shortfall"` (in).

**`pv_rows(roof, *, module=(5.65, 3.72), orient=("L", "P"), uniform=False,
edge=1.5, ridge=None, obstacles=(), pathway=None, gap=0.0, max_rows=8,
time_limit=30.0, name=None)`** puts the most modules on one roof plane, in
rows up the slope. `roof` is `{"rect": (u0, u1, v0, v1), "slope": rise per
foot, "up": "N" | "S" | "E" | "W"}`, obstacles `{"rect", "clear"}`, and
`pathway=True` keeps a 36-in strip clear eave to ridge at the best place.
It returns `{"ok", "count", "rows", "modules" (plan rects), "pathway",
"coverage", "status"}`. **`fire_setbacks(roof, coverage,
sprinklered=False)`** gives `{"applies", "pathway", "ridge" (ft), "why"}`
by CRC R324.6 and CFC 1205.2: 18 in each side of a horizontal ridge at 33 %
coverage or less, 36 in above (66 % sprinklered), none on a roof of 2 in 12
or less. The code readings are unverified. No module of the stage
solves when it loads (the generator's functions solve only when called).
`stdlib/plan_furniture_test.star`, `plan_furnish_test.star`,
`plan_kitchen_test.star` and `plan_roof_test.star` hold the tests.

#### Floor plans as MILPs: `@cadpole/plan_solve.star` and `@cadpole/plan_ask.star`

A free-rectangle model: every room an axis-aligned rectangle at inside faces (`x`, `y`, `w`, `d`, in
feet), every rule a named linear row, exact areas by a binary expansion of
one side. A problem is plain data, built by constructors that record
nothing, so a problem is changed by copying (`@cadpole/plan_ask.star`'s
`apply()` does) and rebuilt for every variant a question needs.

```python
load("@cadpole/plan_solve.star", "floor", "problem", "rect", "room", "solve", "wall")

F = floor("A", zones = {"A": rect(0.5, 30.5, 0.5, 30.5)}, obstacles = {"stair": rect(0.5, 10.5, 0.5, 7.0)},
          fixed = {"lobby": rect(10.5, 14.5, 0.5, 7.0)}, roots = ["lobby"],
          egress = [wall("W", 0.5, (10.0, 30.5)), wall("N", 30.5, (0.5, 30.5))])
R = [room("hall", floor = "A", kind = "hall", value = 0.2, w = (3, 30), d = (3, 30), opens_to = ["lobby"]),
     room("bed1", floor = "A", kind = "bed", area = (100, 150), w = (9.5, 15), d = (9.5, 15), aspect = 1.4,
          opens_to = ["hall", "lobby"])]
P = problem("toy", floors = [F], rooms = R)
plan = solve(P, time_limit = 30)   # a plan value; meta["plan_solve"] has the solver's numbers
```

| Constructor (`@cadpole/plan_solve.star`) | What it is |
| --- | --- |
| `rect(u0, u1, v0, v1)` | A rect on inside faces, feet, u east, v north |
| `floor(key, *, zones, obstacles, fixed, roots, egress, part, zone_walls, columns, must_hold, faces, z, ceiling)` | One floor: zones (their union is the floor), obstacles (a rect, or a dict with a kind and a core; drawn with their walls, so rooms may meet them), fixed rooms doors may open onto (drawn to their inside faces, so a room keeps its partition off one unless it is `open_to` it), the door tree's roots, escape walls, the partition, walls between zones, column lines, must-holds, moving faces |
| `room(key, *, floor, kind, area, w, d, aspect, orient, value, knee, opens_to, open_to, zones, holds, touches, annex, door_gap, hall_min, fixed, kept, ...)` | One room; habitable, sleeping, wet, circulation and dry by kind (`@cadpole/plan.star`'s vocabulary) |
| `wall(side, at, span, *, source)`, `touch(side, at, span, length)`, `hold(rect, among)`, `annex(*, side, w, d, min)` | An escape wall (R310), a line a room runs along, a must-hold, an L-piece |
| `face(name, *, min, max)`, `face_ref(name, offset)` | A moving envelope face; zones' faces and lines may follow it |
| `shared(key, *, floors, w, d)`, `court(key, *, base, floors, w, d, area, rect)` | One footprint through several floors (a free stair); a court as one rectangle from its base up, or, with `rect`, a court of the site written to the plan as it is, so windows may name it |
| `rule(floor, room, what, *, min, max, equals, rect, point, reach, side, at, span, length, family="owner", why)` | A hard rule: a face, `w`, `d`, area; holds; near; clear; touch |
| `stacks(...)`, `structure(...)`, `not_over(floor, room, below, rooms)` | Full-height stacks; bearing lines, beams, transfers and clear spans; acoustics |
| `problem(name, *, floors, rooms, stacks, structure, not_over, faces, shared, courts, rules, sym, hooks, cantilever, drop)` | The problem; `hooks` add their own rows to a model as it is built; `cantilever` grows a floor inside the one below by it (or per floor) |

| Function (`@cadpole/plan_solve.star`) | What it does |
| --- | --- |
| `solve(spec, *, mode="exact", step=0.25, time_limit, gap, max_nodes, fix, pin, drop, floors, objective, anchor, within, widths, broken, name)` | Build, solve, read: a plan value (`normalize`d; its floors' plates are the zones and the core), with `meta["plan_solve"]`: status, feasible, objective, score (exact: value x area, with the curves), gap, bound, nodes, stats, the binding rows (and by family), broken constant rules, layout, areas, doors, beams, lines, transfers, faces, and what the hooks added |
| `options(spec, *, count=3, differ=3.0, among, mirrors=True, ...)` | The best, then each next plan that moves some room `differ` feet past another against every earlier one -- arrangement cuts added between solves; a mirror image counts as one; symmetry rows from the start. Never a Hamming no-good, which returns the same plan |
| `solve_floors(spec, *, order, pins, ...)` | Floor by floor, bottom up, the floors below pinned as found; `meta["plan_solve"]["steps"]` has each step |
| `score(spec, plan)`, `layout_score(spec, layout)` | A plan's (or a layout's) score under a problem |
| `build`, `run`, `read`, `row`, `var`, `binary`, `objective_term`, `room_vars`, `layout_of`, `arrangement_of`, `relations`, `flip_cut`, `drawn_layout`, `arrangement_fix`, `start_of` | The kit hooks and `plan_ask` use: `build(..., fix=, pin=, drop=, shift=, widths=, bounds=, untag=)`, `run(..., start=)`; `start_of(M, M0, sol)` is a MIP start for M from M0's solution, the binaries they share by name |
| `ftin(x)` | Feet-inches; the purity probe |

Modes: `"exact"` (the default: exact areas, the binary expansion's step at
most `step=`, 3 in, both ends reachable); `"proxy"` (areas by secant cuts
and a tangent-plane objective: approximate, several times faster); `"fast"`
(two stages: the proxy's arrangement, sized exactly);
`"grid"` (walls on the floor's column lines). Rows are named
`<family>:<floor>:<room>:<what>`; the families are `zone`, `pair`, `area`,
`size`, `R304`, `R311`, `R307`, `R310`, `shape`, `contain`, `touch`,
`annex`, `door`, `tree`, `sym`, `value`, `plumb`, `struct`, `acoustic`,
`owner`, `envelope`, `cap`, `flip` and `def` (definitions, never dropped or
reported), and a problem's hooks may write their own. `fix=` builds a named
binary as its constant, so a fully fixed arrangement is a pure LP; `pin=`
fixes rooms as numbers; a constant rule that fails is listed in `broken`
(and, unless `broken="skip"`, made an empty row that cannot hold); `drop=`
(families, `"family:floor"`, `"family:floor:room"`) rebuilds without them,
bounds included.

| Function (`@cadpole/plan_ask.star`) | The answer |
| --- | --- |
| `pin(spec, plan, *, sizes="free", doors=True, windows=True, ...)` | The drawing's arrangement held -- the zone each room is in, the sides each pair is apart on, its L-pieces, its door tree, its escape walls -- and its rooms sized within every rule; `sizes="fixed"` holds the drawn sizes too and names what the drawing breaks. `meta["plan_ask"]`: the drawn score, the score, the gain, the rooms that changed, notes |
| `check(spec, plan)` | `pin(sizes="fixed")` |
| `cost(spec, change, *, arrangement="free" \| "pinned", measure, unit, base, plan)` | Two solves: the difference in score, sq ft and `measure`'s unit, and the rooms that move (a tie said so) |
| `explain(spec, *, focus, background, method="rebuild", max_probes=80, room_probes=60, probe_time=5.0, search_time=60.0, ...)` | The rule units (`family:floor`) that clash, then the rooms in each: `{"feasible", "status", "families", "rooms", "undecided", "probes", "answer", "method"}`, irreducible or said not to be |
| `edit(spec, plan, change, *, minimal=True, arrangement="auto" \| "pinned" \| "free", within)` | The change with the fewest feet of wall moved (four faces a room, summed) |
| `duals(spec, plan, *, resolve=0, ...)` | The binding rules' LP duals with every integer fixed -- local to the arrangement, and labelled so; `resolve=n` re-solves the first n with the rule relaxed by a unit, each started at the base's point (`start=`) |
| `apply(spec, change)`, `set_room`, `add_room`, `drop_room`, `set_floor`, `move_zone`, `add_rule`, `drop_family`, `set_problem`, `describe` | Changes as data: `set_room(floor, key, area=(95, 130))`, `move_zone(floor, zone, side, by)` (a zone's face moved outward by `by` feet, its escape walls and touch lines with it), `drop_family("R310:A")`; `apply()` makes the changed problem and leaves the first untouched; `describe` is the purity probe |

A drawing is read to 1/16 in: rooms matched by floor and key; a rect that is
the floor's own fixed room or obstacle is left to it, an obstacle crossing a
rect along one side is cut off it, a piece outside the zones is dropped,
and pieces that join into one rectangle are joined; a room drawn over a
fixed room is open to it, and any other keeps its partition off it. Its door tree is read
breadth first from the drawn doors: each room's parent is its neighbour on
its shortest way to the floor's roots, and a door to the drawn stair counts
as one to the core. `explain()` is Chinneck's additive filter over units,
ordered by `focus=` and then a default order, then a deletion pass, then the
same over the rooms in each unit, each probe a tight model rebuilt without
the units set aside; the background -- zones, walls between rooms,
symmetry, value curves, L-pieces -- is always held and never named.
`method=` picks `"rebuild"` (the default), `"tags"` (each probe one
problem's `drop=`), `"conflict"` (`opt`'s bounded `sol.conflict()`, the
background written untagged so it is the one untagged group, tried last),
`"conflict_families"` (its family pass, then the rooms by `drop=`) or
`"conflict_all"`; the conflict methods stop within `max_probes +
room_probes` probes and `search_time` seconds, inside the CLI's budget.
Every answer is a plan value or a dict of plain numbers, the solver's in
`meta["plan_solve"]` and `plan_ask`'s in `meta["plan_ask"]`; feature names
are `name=` (or `plan_ask_<question>_<problem>`), with a suffix a solve.
`stdlib/plan_solve_test.star` and `stdlib/plan_ask_test.star` hold the
tests, on synthetic floors.

#### Framing: `@cadpole/plan_frame.star` and `@cadpole/plan_spans.star`

A fixed plan's cheapest frame, as one MILP over joist bays. Every output is
**a planning aid, not engineering**, and every sheet says so.

```python
load("@cadpole/plan_frame.star", "reframe", "frame_text")

framed = reframe(plan)                       # the plan, with its frame in it
framed["bearing"]                            # the bearing lines, by floor
framed["meta"]["plan_frame"]["members"]      # every joist bay, beam, header, transfer and stair header, by name
print(frame_text(framed))
```

| Function (`@cadpole/plan_frame.star`) | What it is |
| --- | --- |
| `reframe(plan, menu=None, name=None, floors=None)` | A copy of the plan with `bearing` (the chosen lines), `walls` (its own, plus walls derived from the rooms wherever it has none, each with `kind` and `"bearing"`) and `meta["plan_frame"]`: `status`, `members`, `levels`, `counts`, `score`, `solve`, `checks`, `notes`, `menu`. `floors=` frames those floors alone, as if the others were not there (a kept building, a crawlspace) |
| `frame_menu(menu=None, **kw)` | The member menu and the cost weights, with overrides; pure, and the purity probe. Besides the lumber, loads, beam families and `costs`: `concrete` (concrete walls are founded supports), `span` (`"auto"`, `"u"`, `"v"` or per floor), `slabs` (parts of a level built as structural slabs, left out of the joist model), `grade_z` (a floor at or under it is on grade beyond the floors below it), `skip_kept` (floors of kept rooms are not reframed), `fit_depth` (a depth that does not fit between floors fails), `lines` (`"free"`, or `"plan"`: the plan's own bearing records exactly, the frame as drawn) |
| `frame_members(plan, kind=None, floor=None)` | The members: kinds `joists`, `beam`, `header`, `stair_header`, `transfer`, `doubled_joists` |
| `frame_checks(plan)` | Check rows: transfers (warn, R502.4), a floor standing past the one below by more than the joist depth (warn, R502.3.3), members past the rule (fail), joists that do not fit between floors (fail), and the planning note |
| `frame_text(plan)` | Plain lines: per level the direction and depth, then every member by name, span and size |
| `frame_sheets(S, plan, floors=None, prefix="S", first=1, scale=None, per_sheet=None)`, `frame_set(plan, name=, project=, stamp=, scale=, per_sheet=)` | Framing plans on ARCH D, drawn with `plan_draw`'s `floor_layout()` and `paint()`: up to three levels a sheet, each sheet with the member schedule by level, the red pen and the planning-aid note |

A member is a dict: `name` (`"G B2"`, the level's floor and a mark: J
joists, B beam, H header, S stair header, T transfer, D doubled joists),
`kind`, `floor` (the level it sits in), `level` (`"over G"`), `size`,
`span` (clear, ft), `rule` and `text` (one line); a line member adds
`axis`, `at`, `range`, `a`, `b` and `plf`, and joists `rect`, `spacing`,
`depth` and `max_span`. Each floor has one framing level at its top,
bearing on its own walls, carrying the floor above (40 psf live), a deck
(60 psf) or a roof (20 psf); a `bearing` record's `floor` is the floor
whose walls carry the level. The costs are weights in points, 1 being
about a square foot of 2x10 at 16 in, and `frame_menu(costs={...})`
overrides any of them.

| Function (`@cadpole/plan_spans.star`) | What it is |
| --- | --- |
| `FLOOR_40`, `FLOOR_30` | CRC 2022 Tables R502.3.1(2) (40 psf live) and R502.3.1(1) (30 psf, sleeping areas): every species, grade and spacing the tables list, at 10 and 20 psf dead, as printed |
| `RAFTER_20` | Table R802.4.1(2): rafters, 20 psf roof live load, ceiling attached, L/240 (a flat roof with a ceiling); "note b" (over 26 ft) is read as 26'-0" |
| `HEADERS_EXT`, `HEADERS_INT` | Tables R602.7(1) and (2): girders and headers in exterior and interior bearing walls (span and jack studs) |
| `joist_span(size, spacing, species="DF-L", grade="#2", dead=20, live=40)` | Feet: `joist_span("2x12", 16)` is 16'-6"; `dead=10` gives 18'-1" |
| `rafter_span(size, spacing, species=, grade=, dead=20)` | Feet, Table R802.4.1(2) |
| `header_span(size, wall="interior", supporting="One floor only", width=24, snow=30, braced=True)` | `{"span", "jacks", "table"}`, interpolated by building width (the table's footnote c); `None` past 36 ft |
| `header_for(span, wall=, supporting=, width=, snow=, braced=)` | The least lumber in the table that spans it, or `None` |
| `ijoist_span(depth_in, spacing, use="floor")` | **Rule of thumb**: depth x K, capped; roof x1.15, deck x0.86 |
| `lvl_beam(span, plf, live_plf=None, max_depth=None)`, `steel_beam(...)` | **Rule of thumb**: simple-span bending, shear and deflection with nominal values (LVL 2.0E; A992 at 0.6 Fy on Sx, braced); `W_SHAPES` are recalled AISC properties, **unverified** |
| `joist_options(species, grades, spacings, sawn, ijoists, dead, live)` | Every joist a menu offers, with its floor, roof and deck spans |
| `ftin(x, frac=1)`, `span_ft("16-6")` | Formatting and parsing |

`plan_spans`' purity probe is `joist_span`, `plan_frame`'s `frame_menu`.
`stdlib/plan_spans_test.star` and `stdlib/plan_frame_test.star` hold the
tests.

#### Parti first: `@cadpole/plan_parti.star`, `@cadpole/plan_templates.star` and `@cadpole/plan_rules.star`

A template, a site and a program compiled into one MILP over every floor.
The room stage's fit rows and the measured light are options and off by
default.

```python
load("@cadpole/plan_parti.star", "fit", "compare", "explain")
load("@cadpole/plan_templates.star", "SITE", "PROGRAM", "side_hall", "knob")
load("@cadpole/plan_rules.star", "standard")

plan = fit(side_hall(), SITE, PROGRAM, name = "plan_parti_side_hall")              # about 5 s, proven optimal
alt = fit(knob(side_hall(), "primary_at_street"), name = "plan_parti_primary")
compare([plan, alt])["pairs"][0]["differ"]                                         # 6 of 16 rooms
explain(side_hall(), rules = standard(), name = "plan_parti_x", cheapest = False)  # why it cannot fit: about a minute
```

| Function (`@cadpole/plan_parti.star`) | What it does |
| --- | --- |
| `fit(template, site=SITE, program=PROGRAM, rules=None, time_limit=None, gap=1e-6, name=None, patterns=True, options=None, check=True, max_nodes=None, fit_rows=False, light="assumed")` | One MILP over every floor; returns a plan value in `@cadpole/plan`'s format, clean by `validate()`, with the scorecard in `meta["plan_parti"]`: status, objective, kernel score, pattern, wish and rule terms, gap, bound, nodes, model size, floor and column depths, deck, stair position, gross and built area, measures, each program room's character, the binding rules by family, the measured light, the grafts' findings, and with `rules` a red pen. A template that fails the precheck, or an infeasible problem, returns a plan with no rooms and its reasons (a fit does not run the conflict filter unasked). `fit_rows=True` puts the room stage's fit rows in place of the kernel's fixture depths; `light="measured"` makes a court light the rooms beside it as deep as `SKY_TABLE` says |
| `precheck(template, site, program)` | Before any MILP: every room can take a span (width, bearing lines, fixture depths), reach a door from a room that can touch it, see daylight in some span and stand on a stack; every column on every floor has a room; every program room has a place. Check rows |
| `explain(template, ..., families=None, by_floor=True, cheapest=True, engine="starlark")`, `explain_model(m, ...)` | A deletion filter by rule family, then by family and floor, on one compiled problem (each probe a feasibility solve with `drop=` a tag selector): the irreducible families, the (family, floor) groups, the cheapest group to drop with the objective it leaves, the probes `"undecided"`. `engine="rust"` runs `opt`'s `sol.conflict()` instead, stopped after the families (`families_only=True`) when `by_floor=False` |
| `author(template, ...)` | Precheck, compile, test feasibility, and when infeasible run the filter |
| `options(template, knobs=[...])` | The template's fit and one per knob, and their comparison: options by template and knob, never by no-good cuts |
| `compare(fits)` | Each fit's scorecard line, and for every pair how many program rooms differ in character (floor, depth third, party walls, light sources, what the door opens from) and how far their centres moved; a plan and its mirror image count as one |
| `partis_sheet(S, fits, labels=None, rules=None)` | The comparison sheet, through `plan_draw`'s `compare_sheet`: every floor of each fit at one scale, a scorecard row each (`scorecard_row`) and its red pen (`red_pen`: the audit's marks and the failing checks of the rules); returns the sheet, the check rows and the comparison |
| `massing(plan, name=, at=)` | Any plan value as blocks for the viewer: each indoor room from its floor to its ceiling, coloured by kind, decks as slabs, courts open |
| `resolve`, `compile_template`, `plan_of`, `slim`, `measured_light`, `character`, `fit_table`, `grid`, `solve`, `ftin`, `sqft`, `FAMILIES`, `CODE`, `FIT_SETS` | The kit; `CODE` holds the kernel's constants, which `options=` overrides; `grid` is the purity probe |

`@cadpole/plan_templates.star`: `SITE`, `PROGRAM`, `PROGRAM_KEYS`,
`EXTRA_ROOMS`, `program_with(...)`, `LIBRARY`, `library(keys)`, the four
partis `side_hall(...)`, `cross_stair(...)`, `court(...)` and
`open_core(...)` (each with `mirror=`, `floors=3 | 4`,
`split=[(col, u)]`, `court=True | False | "optional"`,
`light_well={"side", "y", "d"}`), and the transforms `mirror`,
`split_column`, `with_light_well`, `without_court`, `optional_court`,
`short_columns`, `floors_only` (some consecutive floors, for drafting and
tests), `single_floor`, `door_rules`, `knob`, `copy_template`, `set_room`,
`room_of` and `tidy`. Its purity probe is `side_hall`. A template is plain
data (the module's docstring has it in full):

```python
{"key", "name", "idea", "precedent",
 "lines": [3.7, 7.6, 18.2],        # partition centre lines across the 24-ft inside width: the column grid
 "floors": ["G", "M", "U"],        # bottom up
 "stair": {"type": "side", "col": 0} | {"type": "cross", "land_w": 0, "land_e": 3} | None,
 "wells": [{"pre", "col", "floors", "rear", "side", "lit_depth"}],
 "bay": {"host": {floor: room}, "u0", "u1", "proj"},
 "bearing": [line], "stacks": [line, ...], "orders": [(floor, a, b)],
 "patterns": {"two_sided": ("weight", 30.0), "entrance_turn": ("weight", 10.0)},
 "rooms": [placements],
 "doors": {"must": [(room, [others])], "wish": [(a, b, weight | "hard")], "ban": [(a, b)]},
 "short": [col | (floor, col)], "fixed": {room: {"span", "y", "d"}}, "fix_depth": {floor: depth},
 "knobs": {name: function(template, value)}}
```

A placement names a program room (its sizes from the program) or a
template-owned room (a hall, a closet, a store, the stair, a court, with its
own sizes), its floor, the spans it may take, the rooms it may open off
(`parents`, any order; `"ROOT"` the street, `"stair.top"` the arrival
landing), its facade pins and any size it overrides; `role` is the kernel's
class (hab, sleep, wet, circ, serv, garage, stair, void) and `kind` the plan
format's. Each room has one access door from its parents, which hangs a
floor's rooms in a tree off the street or the landing; `must` adds required
doors, `wish` weighted ones (taken when worth more than a door's cost in
the objective, 0.5), and `ban` forbids pairs. Kept rooms (`kept=True`, or pinned in
`fixed`) are exempt from the design rules; the physical rows stay. Rules are
named `<family>:<floor>:<room>:<what>`, the families `tiling`, `order`,
`facade`, `fixtures`, `shape`, `program`, `stair`, `well`, `access`,
`doors`, `light`, `stack`, `floors`, `cap`, `garden`, `pattern`, `fixed`,
`site`, `fit` and each menu rule's own; the model's definitions (`def`) and
symmetry rows (`sym`) carry no tags.

`@cadpole/plan_rules.star`: `menu()`, `rule(name, hard=, weight=, **args)`,
`standard(**overrides)`, `architect()`, `rule_families(rules)` and
`check_rules(plan, rules)`, which checks the same rules on any plan value.
Its purity probe is `rule`. The standing set:

| Rule | Class | The exploit it closes |
| --- | --- | --- |
| `garage_size` | size, 2 a sq ft past 11'-6" x 22'-0" | a column grid that gives the garage 15'-8", 61 sq ft over; hard, the filter names it, and a column split at 12'-6" fixes it |
| `hall_width` | typology | a 24'-0" x 4'-10" cross hall soaking up depth; held to 4'-6" |
| `storage_cap` | size, 1 a sq ft over the cap | an 80-sq-ft mudroom against a 60-sq-ft cap (stores 100, workshops 150) |
| `well_band` | typology | a well only in the middle third of the floor, lighting a habitable room; a court moved to the street is flagged |
| `pantry_by_kitchen` | typology | a pantry off the hall, a 69-ft walk from the kitchen; the rule costs that floor 31 points |
| `stack_day` | typology | a stack in a living room is flagged; the kernel's stacks run on column lines and never cross a room |

The architect's rules are `garage_door`, `no_garage_only`, `wet_off_hall`
and `bed_off_hall` (door etiquette, weighted 10), `egress_facade` (R310.1's
yard-or-court reading, unverified) and `bed_width` (8'-6" across); `not_over` keeps a wet room off a living room or bedroom
on the floor below, or any pair of kinds. `examples/plan_parti_fits.star`
stores the four fits, proven optimal, as plan values, and
`examples/plan_parti_compare.star` draws their comparison sheet without
solving (`cadpole export examples/plan_parti_compare.star --run-exports
--out-dir /tmp`), and with `-p refit=<parti>` (or `<parti>:<knob>`) refits
one and prints its line of the fits file. `stdlib/plan_parti_test.star`,
`plan_parti_explain_test.star`, `plan_rules_test.star` and
`plan_templates_test.star` hold the tests.
