# Cadpole Starlark API

This is the reference for Cadpole's scripting language: the globals, the value
types and the standard library. Scripts are the model: everything the UI shows
is derived from evaluating them. The API favours explicit names, keyword
arguments and diagnostics that point at a line.

It is the single place a Cadpole signature is written. The machine-readable
form is `cadpole doc --json` (and `Engine.api_doc()` in wasm), generated from
the doc comments of the implementation.

Related: `docs/design.md` (feature tree, topological naming, kernel,
assemblies), `examples/*.star`.

Signatures in `cadpole doc` use Python's markers: parameters before `/` are
positional-only (`cylinder(r=..., h=..., /, *, d=..., height=...)` is called
`cylinder(5, 10)`, not `cylinder(r=5, h=10)`), and parameters after `*` or
`*args` are keyword-only. A value such as `math.pi` or `XY` is listed as
`name: type` (`"constant": true` in `--json`), not as a call.

## Contents

Cadpole does two kinds of work on one engine, and the reference is three
documents:

| Document | What it covers |
| --- | --- |
| This one | **The shared core**: the language, parameters, sketches, solids, booleans, measurement, materials, layers, libraries, imports; then output (exports, printing, drawings, sheet sets) and tests. Both kinds of work use all of it. |
| [`parts.md`](parts.md) | **Parts and assemblies**: mechanical parts, mechanisms, things to print or machine. A first example, the commands, then assemblies ([§P4](parts.md#p4-assemblies)), gears ([§P5](parts.md#p5-gears-cadpolegearsstar)), springs, threads and fasteners ([§P6](parts.md#p6-springs-threads-and-fasteners-cadpolemechanicalstar)). |
| [`buildings.md`](buildings.md) | **Buildings and plans**: massing, sites, floor plans, sun and shadow. A first example, the commands and units, then feet, inches and massing ([§B4](buildings.md#b4-feet-inches-and-massing-cadpolebuildingstar)), terrain ([§B5](buildings.md#b5-terrain)), lots and site data ([§B6](buildings.md#b6-lots-and-site-data-cadpolesitestar)), sun and shadow ([§B7](buildings.md#b7-sun-and-shadow)), sightlines ([§B8](buildings.md#b8-sightlines-and-views)), choosing values with `opt` ([§B9](buildings.md#b9-choosing-values-the-opt-namespace)) and floor plans ([§B10](buildings.md#b10-floor-plans-cadpoleplan)). |

A section reference is written §n: plain numbers are sections of this
document, §P numbers are in `parts.md` and §B numbers in `buildings.md`. A
script for a part never needs `buildings.md`; a script for a building uses
this document exactly as a part does.

**The shared core**

| Section | What is in it |
| --- | --- |
| [§1](#1-principles) Principles, [§2](#2-conventions) Conventions | how the language behaves and how calls are spelled |
| [§3](#3-core-types) Core types | `Plane`, `Sketch`, `Curve`, `Body`, `Face`, `Edge`, `Material`, `Layer`, `Field` and the rest |
| [§4.1](#41-parameters) Parameters, [§4.2](#42-planes) Planes | `param`, `units`, the standard planes |
| [§4.3](#43-sketching) Sketching | sketches, paths, constrained sketches, the 2D toolkit |
| [§4.4](#44-solids) Solids, [§4.5](#45-booleans) Booleans | `extrude`, `revolve`, `loft`, `sweep_along`, `fillet`, `chamfer`, `emboss`, patterns; `union`, `subtract`, `intersect` |
| [§4.6](#46-transforms-and-copies) Transforms, [§4.7](#47-organic-and-field-bodies) Field bodies | `translate`, `rotate`, `mirror`; signed-distance blends |
| [§4.8](#48-queries-and-selectors) Queries, [§4.9](#49-measurement) Measurement | `faces()`, `edges()`; `distance`, `clearance_map` |
| [§4.10](#410-the-math-namespace) `math`, [§4.11](#411-formatting-numbers-formatvalue-spec) `format` | numbers |
| [§4.12](#412-materials-and-rendering) Materials and rendering, [§4.13](#413-layers) Layers | appearance, `render_defaults`, display layers, glTF |
| [§4.14](#414-libraries-and-load) Libraries, [§4.15](#415-assets-imports-and-text) Assets and imports | `load()`, the standard library's modules; SVG, DXF, STEP, mesh, text |
| [§4.16](#416-vocabularies) Vocabularies, [§4.17](#417-animations) Animations | the closed sets of names; keyframes and cameras |
| [§5](#5-naming-and-stability-rules) Naming and stability | feature ids, body ids, topological names |

**Output**: what a script writes, for either kind of work.

| Section | What is in it |
| --- | --- |
| [§6.1](#61-exports-and-printing) Exports and printing | `export` (STL, OBJ, 3MF, STEP), `print_check`, `orient_for_print`, `paint`, `split`, `printer` |
| [§6.2](#62-printing-in-pieces-cadpoleprintstar) Printing in pieces | `@cadpole/print.star`: pegs, sockets, story bands |
| [§6.3](#63-drawings) Drawings | `drawing`, `export_svg`, `export_dxf`, `export_pdf` |
| [§6.4](#64-sheet-sets) Sheet sets | `sheet_set`, title blocks, plan, section and elevation views, `sweep_table` |

**Tests and assistants**

| Section | What is in it |
| --- | --- |
| [§7.1](#71-model-tests) Model tests | `test`, sweeps, monotone claims, the assertions |
| [§7.2](#72-assistant-hooks-and-diagnostics) Assistant hooks and diagnostics | `selection`, `warn`, `fail`, the `assert_*` family |
| [§7.3](#73-diagnostics-reference) Diagnostics reference | severities, spans, message shape, exit codes |
| [§8](#8-worked-examples) Worked examples | a planter, a bracket, a box with a lid, a vase, a dimensioned plate |

---

## 1. Principles

1. **The script is the model.** There is no hidden state: re-running the
   script from scratch reproduces the model exactly. The UI selects things; it
   never creates features.
2. **Everything is named.** Every call that produces geometry records a
   *feature* with an id (`name=` if given, else `<kind>_<n>`). Bodies, faces,
   edges and sketch entities have stable names derived from how they were
   made (§5). Selections are names, not indices.
3. **Values are immutable snapshots; features are the record.** A `Sketch`
   or `Body` value never changes after creation. "Modifying" operations
   (`translate`, `union`, `.rect()`) return a new value and append a feature.
   Inputs consumed by an operation are hidden, not deleted, so the tree is a
   full history.
4. **Units are fixed:** millimetres for length, degrees for angles, mm² and
   mm³ for area and volume. No unit objects, no implicit conversion. The
   document's `units(...)` (§4.1) only affects presentation: what
   `cadpole inspect`, `distance()`, the drawings and the web UI *write*, never
   what a number *is*. `ft(45)` in `@cadpole/building.star` is `45 * 304.8`.
5. **Integers stay integers.** A `param("count", 4)` returns an `int`; counts
   and indices accept `int` or an integral `float` (`4.0`) and reject `4.5`.
6. **Errors are diagnostics with spans.** A failing call stops evaluation and
   reports `path:line:col` plus the feature it belongs to. `warn()` reports
   without stopping. A model with errors is still returned (all features up to
   the failure) so the UI can show where things went wrong.
7. **No I/O, no time, no randomness.** Scripts cannot read files, fetch URLs,
   or observe the clock. `export()` *records intent*; the host decides
   whether and where to write. Randomness is seeded and part of the feature
   record (`field.noise(seed=...)`).
8. **Deterministic and bounded.** Same script + same overrides = same model,
   same names, same mesh (given the same tolerance). Evaluation has a step,
   memory and time budget set by the host (see `docs/security.md`).
9. **Keyword-only for anything optional.** Positional arguments are reserved
   for the one or two things a call is obviously about (`extrude(sketch,
   distance)`); everything else is `name=value`. This keeps scripts readable
   when an assistant edits one argument.
10. **Small surface, composable.** One general call with clear keywords is
    preferred over many special-purpose calls. Sugar exists where it reads
    better (`box`, `cylinder`).

## 2. Conventions

- **Arguments.** At most two positionals (`box(w, d, h)` is the listed
  exception). `name=` is always optional and always the feature id. `kind=`
  selects variants. `center=` is always a point.
- **Reserved keywords.** `size` is millimetres (pixels only in image sizes);
  `scale` is a unitless factor.
- **Angles and lengths.** Angular tolerances are `within=` (degrees) on
  `faces()`/`edges()`; `tolerance=` is always a length in mm. Passing
  `tolerance=` for an angle is an error naming `within=`.
- **Builtins.** Cadpole never shadows a Starlark builtin. The imperial parser
  is `length("12'-6\"")`, not `len(...)`; there is no `deg()` because angles
  are already degrees; unit helpers are functions (`ft(4)`), never constants.
- **Libraries.** Nothing is auto-loaded: every library is
  `load("@cadpole/<name>.star", ...)`.
- **Attribute names.** Starlark attributes drop the `_id` suffix and the
  qualifier the value's type implies (`ref.feature`, not `ref.feature_id`;
  `face.kind`, not `face.surface_kind`). Proto and JSON keep the long names.

## 3. Core types

All types are opaque Starlark values with attributes (read with `.attr`) and
methods. `str(value)` gives a short human description; `type(value)` gives the
type name below. Values compare by identity except where noted.

`Terrain`, `Sun`, `Shadow`, `ShadowStudy`, `Sightline` and `SightlineFan` are
the values of building work (`docs/buildings.md`); a script for a part never
meets them.

### Plane

A sketch plane: origin, unit normal and in-plane x direction. The in-plane y
direction is `normal × x_dir` (right-handed).

| Attribute | Type | |
| --- | --- | --- |
| `origin` | `(x, y, z)` | |
| `normal` | `(x, y, z)` | unit |
| `x_dir` | `(x, y, z)` | unit, perpendicular to `normal` |
| `y_dir` | `(x, y, z)` | derived |

| Method | Returns | |
| --- | --- | --- |
| `offset(distance)` | Plane | parallel plane moved along `normal` |
| `to_world(x, y)` | `(x, y, z)` | sketch coordinates to world |

Built-in planes: `XY` (normal +Z), `XZ` (sketch y → world Z, normal −Y),
`YZ` (sketch x → world Y, sketch y → world Z, normal +X). These three are
constants, not features.

### Sketch

A plane plus closed loops and open construction paths in 2D sketch
coordinates. Constraints are a separate value, `ConstrainedSketch` (below). Loops are classified by
nesting: outermost loops are material, loops inside them are holes, loops
inside holes are islands (even-odd). A loop is inside another only when all
of it is and no edge of it crosses the other's: two loops that cross -- a
plus sign drawn as two bars -- are two outers, not a hole of nothing that
vanished, and `area()` refuses them by name. Loop orientation does
not matter; Cadpole normalises it. Every `sketch()` call is a feature of kind
`sketch`; adding a loop updates that feature's arguments and returns a new
`Sketch` value with the same feature id. The values of a chain share their
loops, so appending in a long loop is linear.

| Attribute | Type | |
| --- | --- | --- |
| `id` | str | feature id |
| `plane` | Plane | |
| `loops` | int | number of loops |
| `paths` | int | number of open paths: chains that bound nothing -- what `polyline()` and `add(path, close=False)` draw, and what an import brings in (a DXF centre line, an unfilled SVG path). They are never extruded, and `export_svg`/`export_pdf`/`export_dxf` write them as the open strokes they are |
| `names` | list[str] | one per loop; imports fill these from the source file's own ids (`import_svg_1/badge/path[3]/loop[0]`), hand-drawn loops leave them empty |
| `path_names` | list[str] | one per construction path |

Loop methods (all return a new Sketch):

| Method | Notes |
| --- | --- |
| `rect(w, h, center=(0, 0))` | axis-aligned rectangle |
| `circle(r, center=(0, 0))` | full circle (two arcs internally) |
| `polygon(points)` | closed polygon through `[(x, y), ...]`, ≥ 3 points |
| `ngon(n, r, center=(0, 0), rotate=0)` | regular polygon inscribed in radius `r`; `rotate` in degrees |
| `rounded_rect(w, h, r, center=(0, 0))` | rectangle with corner arcs of radius `r` (a plain rect when `r` is 0) |
| `slot(length, width, center=(0, 0))` | stadium: `length` along x, `width` along y |
| `spline(points, tension=0.5)` | closed Catmull-Rom spline through the points |
| `add(path, *, close=True)` | a loop built with `path(...)` (§4.3), closed with a line back to its start if needed; `close=False` adds it as an open path instead, arcs kept |
| `polyline(points, *, closed=False)` | an open polyline through `[(x, y), ...]`, ≥ 2 points: a contour, a grade line, a property line. `closed=True` is `polygon(points)` |
| `curves()` | every loop as a closed `Curve` in world space, as drawn, in loop order; each carries the sketch's id. The bridge to a `loft` through sections that are not parallel |
| `points(*, tolerance=None)` | every loop as a list of `(x, y)` points in the sketch's own coordinates, in loop order: a polygon's corners as drawn, an arc or a spline sampled to `tolerance` (the document's by default). For reading geometry back — a section's outline, a footprint's corners — where a `Curve` would be the wrong shape |
| `area()` | the area the loops enclose, in mm²: every region's material less its holes, by nesting. Exact for lines and arcs; a spline is sampled to the document tolerance; an empty sketch is 0; loops that cross are refused by name |

Text is the global `text(...)` (§4.15), which returns a Sketch.

The 2D toolkit, each returning a new Sketch on the
same plane and recording a feature whose input is this sketch; the global
forms and the rules are in §4.3, "The 2D toolkit":

| Method | Notes |
| --- | --- |
| `offset(d, *, join=join.round, name=None)` | every loop offset by `d`: positive grows the material, negative shrinks it; lines stay lines, arcs stay arcs, self-intersections are cut away |
| `union(other, *, name=None)`, `subtract(other, *, name=None)`, `intersect(other, *, name=None)` | 2D booleans with a sketch on the same plane; the globals are `union2d`, `subtract2d`, `intersect2d` |
| `fillet2d(r, *, corners=None, name=None)` | corners rounded by arcs tangent to both segments; `corners=` by name, index or `(loop, corner)` |
| `chamfer2d(d, *, corners=None, name=None)` | corners cut back `d` along both segments |
| `mirror2d(*, axis=(0, 1), origin=(0, 0), keep=True, merge=True, name=None)` | loops reflected across a line in the sketch plane |
| `pattern2d_linear(count, spacing, *, merge=False, name=None)`, `pattern2d_circular(count, *, center=(0, 0), angle=360, merge=False, name=None)` | repeated loops, `<loop>/copy[k]` |
| `corners()` | the corner names, `<loop>/corner[k]`, that `fillet2d`/`chamfer2d` select from |

### Curve

A chain of segments in **world** coordinates — lines, three-point circular
arcs, Béziers and Catmull-Rom splines (one cubic Bézier per span, clamped
ends), exactly the vocabulary of a sketch `Path` but with `(x, y, z)` points,
so the chain need not lie in any plane. Built by the global `curve(start)`,
by `Path.on(plane)` (a sketch path lifted into space) or by `Sketch.curves()`
(a sketch's loops). Immutable: every method returns a new value with the same
feature id, and each segment added is recorded on the feature. It is the one
curve type in the language: `loft` takes a closed `Curve` as a section, and
`sweep_along` takes one as its path. `helix(radius, pitch, turns,
...)` builds one whose single segment is a helix stored in closed form.

| Attribute | Type | |
| --- | --- | --- |
| `id` | str | the feature: its own `curve` feature, or the sketch it was lifted out of |
| `start`, `end` | `(x, y, z)` | the end is the start again once closed |
| `segments` | int | |
| `closed` | bool | |
| `length` | float | mm |
| `plane` | Plane or `None` | the best-fit plane when every control point lies within the kernel's tolerance (1e-6 mm) of it — which is exactly when `cap="plane"` can close it; `None` for a curve that is genuinely three-dimensional. Its normal follows the drawing direction (right-hand rule), its origin is the centroid |
| `start_plane`, `end_plane` | Plane | the planes perpendicular to the curve at its two ends under the rotation-minimising frame: where a `sweep_along` profile is drawn, and where it arrives with the same orientation. `x_dir` is the frame's normal |

| Method | Returns | |
| --- | --- | --- |
| `line(to)` | Curve | straight to `(x, y, z)` |
| `line_by(delta)` | Curve | straight by `(dx, dy, dz)` |
| `arc(to, *, via)` | Curve | the circle through the current point, `via` and `to`; collinear points are a diagnostic |
| `bezier(to, *, controls=[c1[, c2]])` | Curve | quadratic or cubic; an exact NURBS edge |
| `spline(points, *, tension=0.5)` | Curve | Catmull-Rom through the points from the current one, one cubic per span, ends clamped as `Path.spline` clamps them — so two sections built from the same station list pair station for station |
| `close()` | Curve | closed; a straight segment back to the start is added only if there is a gap. Nothing can be added afterwards |
| `reverse()` | Curve | run the other way; a closed curve keeps its seam. The fix when a loft warns that a section is drawn the opposite way round from its neighbour |

### Wrapped

A planar sketch laid on one face of a body: what `wrap(sketch, face, ...)`
returns and what `imprint` and `emboss` take in place of a sketch. It holds
the face, the method (`project` or `unroll`), the frame the sketch was placed
in, the sketch in that frame, and the curve each loop makes on the surface —
exact edges where the kernel has them, cubic Béziers fitted within 1e-7 mm
elsewhere, with the largest measured deviation on the value and on the
`wrap` feature. Immutable; the `wrap` feature records the body, the face,
the method, the loop count and the deviation, and the kernel's notes (how
many edges were exact, how many fitted, a loop united with a neighbour).

| Attribute | Type | |
| --- | --- | --- |
| `id` | str | the `wrap` feature |
| `face` | str | the face's full name, as the kernel names it (`cyl/side#0`; the alias the call used resolves to it) |
| `method` | str | `"project"` or `"unroll"` |
| `kind` | str | the face's recognised surface class (`plane`, `cylinder`, `cone`, `sphere`, `torus`, `other`) |
| `deviation` | float | mm: the largest distance a fitted edge was measured to be from the true curve; `0` when every edge is exact |
| `loops` | int | |
| `names` | list of str | one per loop: the loop's own name (`label/glyph[3]/loop[0]` for text) or its index |

| Method | Returns | |
| --- | --- | --- |
| `curves()` | list of Curve | one closed `Curve` per loop, on the face: lines, arcs and Béziers where the edge is exact, a helix in closed form for a line developed round a cylinder, cubic Béziers for the rest — a `loft` section or a `sweep_along` path |

### Terrain

A ground surface `z = f(x, y)` over a rectangle (or the points' convex
hull): what `terrain(...)` and `terrain_from_contours(...)` return (§B5).
Either the Delaunay triangulation of the spot elevations (`method=terrain.tin`
— through every point, a mesh body) or a least-squares bicubic B-spline
through them (`terrain.fit` — smooth, an exact B-rep body). A value, not a
body: the `terrain` feature records the points, the method, the extent and
the triangle count or the net and its residual, and makes no body until
`.body()`, which is one kernel op keyed by everything that decides it.
`offset_up` and `clip` return new values with the same feature id;
nothing is recorded. Immutable. Every length is millimetres; grades are
rise over run.

| Attribute | Type | |
| --- | --- | --- |
| `id` | str | the `terrain` feature |
| `method` | str | `"tin"` or `"fit"` |
| `points` | int | spot elevations given (duplicates in the plane are dropped from a TIN and counted on the feature) |
| `extent` | ((x0, y0), (x1, y1)) | the rectangle the surface covers (after a `clip`, the clip) |
| `z_min`, `z_max` | float | the elevation range: a TIN's points, or the fit's control net, which bounds it; raised with the surface |
| `offset` | float | how far `offset_up` has raised it |
| `triangles` | int or None | a TIN's triangle count |
| `control`, `spans` | (int, int) or None | a fit's control points and knot spans, along x and along y |
| `residual`, `max_residual` | float or None | a fit's rms and largest residual over its points, mm |

| Method | Returns | |
| --- | --- | --- |
| `z_at(x, y)` | float | the elevation; an error off the surface (the hull, or the extent), naming the terrain and its extent |
| `slope_at(x, y)` | float | the grade, `0.25` for 25 %: the TIN's triangle or the patch's analytic gradient |
| `downhill_at(x, y)` | (dx, dy, 0) or None | the unit direction of steepest descent; `None` on flat ground |
| `average_grade(polygon)` | float | the slope of the least-squares plane through the surface sampled on ~400 points inside the polygon (a list of `(x, y)` or a Sketch's first loop): "a 25 % lot" |
| `z_range(polygon)` | (lo, hi) | the elevation range over the polygon, from the same samples plus the polygon's vertices |
| `contours(interval)` | list of Curve | one polyline per contour line at every multiple of `interval`: exact on a TIN's edges, marched over a 96 × 96 sampling of a fit; at most 500 lines |
| `contour_sketch(interval, *, name=)` | Sketch | the same lines as construction paths on XY, named `contour[<z>]` (`#k` for the k-th at one level): a site plan for `export_svg`/`export_dxf`; records a `sketch` feature |
| `offset_up(h)` | Terrain | the surface raised `h`: every vertex or every control point moved — exact, and what "no higher than 30 ft above grade at every point" is |
| `clip(extent)` | Terrain | the same surface over a smaller rectangle: a fit split by knot insertion, a TIN's triangles clipped, `z_at` unchanged wherever both are defined; refused outside the extent |
| `body(*, base=None, tolerance=None, name=None)` | Body | the slab from the surface down to `z = base` (default a tenth of the relief below the lowest point, half a metre at least): a TIN's is a mesh with `<name>/top`, `/bottom`, `/side`; a fit's an exact B-rep with `<name>/top` (the patch), `/bottom`, `/side[0]`…`[3]` (south, east, north, west). `tolerance=` is the fitted slab's own chord tolerance (`Body.tessellation`; a site is metres, and 0.05 mm makes a few hundred thousand triangles of a hillside), ignored by a TIN whose triangles are the surface |

### Drawing2D

What `import_dxf` returns: an ASCII DXF parsed once — layers, entities in
millimetres, labels and points — that the script questions layer by layer.
Every view is a pure function of the parsed drawing, so asking twice costs
nothing and a memo hit answers from the same drawing.

| Attribute | Type | Notes |
| --- | --- | --- |
| `id` | str | the import feature's id |
| `units`, `unit_source` | str | the unit the file was scaled from (`"ft"`) and where it came from: `"$INSUNITS"`, `"units="` or `"assumed"` |
| `scale` | float | millimetres per drawing unit |
| `entities` | int | entities kept, `INSERT`s expanded |
| `layer_names` | list[str] | the layers the import kept, in file order |
| `version` | str | `$ACADVER` and its release, `"AC1024 (2010)"` |
| `extents` | `((x,y,z),(x,y,z))` or None | `$EXTMIN`/`$EXTMAX` as the header declared them, in mm |

Methods: `layers()`, `sketch(layer=, closed=, plane=, at=, align=,
tolerance=)` → Sketch, `curves(layer=, tolerance=)` → list[Curve],
`points(layer=)`, `texts(layer=)`, `spot_elevations(layer=, pattern=)`,
`contours(layer=, tolerance=)`, `bbox()` — each described under
`import_dxf` in §4.15. A Drawing2D is accepted wherever a Sketch is, as its
`sketch()` with the defaults.

### ConstrainedSketch

A sketch built from *dimensions* rather than coordinates: named entities, named
constraints, and a numerical solve that turns them into the loops above. It is
a second value type rather than a mode of `Sketch`, because building one is
stateful (`s.line(a, b)` adds to `s`) while a `Sketch` is an immutable snapshot.
`extrude` and `revolve` accept either type.

```python
w = param("width", 80)

s = constrained_sketch(XY, name="plate_profile")
bl = s.point(0, 0, name="bl", fix=True)      # the anchor
br = s.point(w, 1, name="br")                # a starting guess, not the answer
tr = s.point(w + 1, 40, name="tr")
tl = s.point(-1, 39, name="tl")
bottom = s.line(bl, br, name="bottom")
right = s.line(br, tr, name="right")
top = s.line(tr, tl, name="top")
left = s.line(tl, bl, name="left")

s.constrain("horizontal", bottom)
s.constrain("vertical", right)
s.constrain("horizontal", top)
s.constrain("vertical", left)
s.constrain("distance", bl, br, w, name="width")
s.constrain("distance", br, tr, 40, name="depth")

hole = s.circle(s.point(10, 20, name="hc"), 2.75, name="hole")
s.constrain("distance", "hc", left, 10, name="hole from the left edge")
s.constrain("distance", "hc", bottom, 20, name="hole from the bottom")
s.constrain("radius", hole, 2.75, name="bolt")

assert_eq(s.dof(), 0)
plate = extrude(s, 6, name="plate")
```

Two lines given the same point *share* that corner — a line owns no
coordinates — so a closed outline needs no `coincident` constraints, and only
arcs and separately built chains do.

| Method | Returns | |
| --- | --- | --- |
| `point(x, y, *, name, fix=False, construction=False)` | SketchEntity | `(x, y)` is a starting guess unless `fix=True`, which pins it |
| `line(a, b, *, name, construction=False)` | SketchEntity | between two points of this sketch |
| `circle(center, r, *, name, construction=False)` | SketchEntity | `center` is a point of this sketch |
| `arc(center, r, *, start, end, name, construction=False)` | SketchEntity | counter-clockwise from `start` to `end` **degrees** |
| `constrain(kind, a, b=None, c=None, *, name=None)` | ConstrainedSketch | one constraint; returns the sketch, so calls chain |
| `close(entities, *, name=None)` | ConstrainedSketch | declare that these curves bound one loop |
| `solve()` | ConstrainedSketch | solve now rather than at first use |
| `dof()` | int | degrees of freedom left; 0 is fully constrained |
| `at(entity)` | `(x, y)` | solved position of a point, `.start`, `.end` or a centre |
| `radius_of(entity)` | float | solved radius of a circle or an arc |
| `entity(name)` | SketchEntity | look an entity up by name |
| `entities()` / `constraints()` | list[str] | names, in creation order — the labels diagnostics print |
| `sketch()` | Sketch | the solved geometry as an ordinary sketch |
| `plane()` | Plane | |

`ConstrainedSketch.id` is the feature id, as for `Sketch`.

A **SketchEntity** is one named entity. Its attributes are `name`, `kind`
(`"point" | "line" | "arc" | "circle"`) and the point-like parts `start`, `end`
and `center`, which are what a constraint refers to: `s.constrain("coincident",
left.end, fillet.start)`. Anywhere an entity is expected, its *name* works too
(`s.constrain("horizontal", "bottom")`).

**Constraint kinds.** `kind` is the first argument to `constrain` and is one of
the fifteen below; `a`, `b` and `c` are its operands. Where a point is wanted,
a point entity, an `entity.start` / `.end` / `.center`, or a circle or arc
(meaning its centre) are all accepted.

| `constrain(...)` | Operands | |
| --- | --- | --- |
| `"coincident", a, b` | two points | |
| `"horizontal", line` / `"vertical", line` | a line | |
| `"distance", a, b, d` | two points, or a point and a **line** | a line operand means the perpendicular distance to it, which is how something is dimensioned from an edge; the point keeps the side of the line it started on |
| `"angle", line_a, line_b, degrees` | two lines | counter-clockwise from `a` to `b` |
| `"parallel", a, b` / `"perpendicular", a, b` | two lines | |
| `"tangent", a, b[, at]` | two curves, at least one curved | with `at` (a point the two already share) the residual becomes "the tangents agree here", which is the well-conditioned form for a fillet or a slot end |
| `"equal", a, b` | two lines (length) or two radial curves (radius) | |
| `"symmetric", a, b, axis` | two points and a line | |
| `"fix", entity` | any entity | holds every parameter it has at its current value |
| `"radius", curve, r` | a circle or an arc | |
| `"concentric", a, b` | two radial curves | |
| `"midpoint", point, line` | | |
| `"on_curve", point, curve` | | the infinite line, or the full circle an arc lies on |

`name=` labels the constraint; without it the label is the kind and the operand
names — `distance(a, b)`, `horizontal(bottom)` — and a duplicate label gets
`#n` appended. Labels are what every diagnostic prints, so they are worth
setting on dimensions.

**Construction geometry** (`construction=True` on any entity) takes part in the
solve and never becomes material: a centreline, a symmetry axis, a bolt circle.

**Solving.** Building the sketch is lazy; the solve happens the first time the
sketch is *used* — extruded, revolved, or queried with `dof()`, `at()`,
`radius_of()` or `sketch()` — and the result is cached until the sketch
changes, so extruding one sketch twice solves once. The solve starts from the
coordinates in the script and is deterministic: the same script always gives
the same answer, and that answer is the solution nearest the starting guess
(which is what picks one of the several a constraint system usually has). A
dimension driven by `param()` re-solves whenever the parameter changes, because
the document is re-evaluated — that is what makes the sketch parametric.

**Loops.** After the solve, non-construction curves are chained into closed
loops by position, and each circle is a loop on its own; the loops then nest
into regions exactly as `rect()`'s do (§3, `Sketch`). Chaining starts at the
first entity in creation order, so it is deterministic. `close([...])` declares
a loop explicitly when the curves could be chained more than one way. A chain
that does not close becomes a **construction path** — reported, never
extruded — with a warning saying how far from closing it was.

**Diagnostics** (§7.3) come with the span of the call that caused them:

| | Severity | Message |
| --- | --- | --- |
| Degrees of freedom left | `info` | `sketch "plate" is under-constrained: 3 degrees of freedom remain (free: b, top)` |
| A constraint that adds nothing | `warning` | `redundant constraint: "top width" is already implied by "width", "vertical(left)", "vertical(right)"`, on the line of that `constrain(...)` call |
| Constraints that cannot both hold | `error` | `conflicting constraints: "top width" contradicts "width" by 5 mm`, on the line of that `constrain(...)` call, plus an error on the operation that needed the geometry |
| The solve did not converge | `error` | `the solver did not converge: the steps became too small to make progress after 200 iterations, worst constraint error 3.4 mm` |

Under-constrained is normal while modelling and is only information; a sketch
that reports degrees of freedom still extrudes, using the solution nearest the
coordinates in the script. Conflicting constraints stop the operation instead
of extruding a least-squares compromise.

Worked examples: `examples/constrained_bracket.star` (a plate whose holes are
dimensioned from its edges) and `examples/constrained_lprofile.star` (an
L-profile whose inner corner is a fillet held on by two tangencies).

Sketch entity names: `sketch_1/loop[0]/seg[2]` (§5). A `Sketch` value can be
reused by several features (`extrude` twice with different distances).

### Body

A solid, in one of two representations: an exact B-rep solid (one connected
shell) or a mesh with named face groups. `body.kind` reports
`"brep" | "mesh"`; a boolean produces a mesh body when it is routed to the
mesh kernel (§4.5). A `Field` is a value, not a Body kind (§4.7).

| Attribute | Type | |
| --- | --- | --- |
| `id` | str | `<feature id>#<n>`, `n` counting bodies across the document from 0 |
| `feature` | str | the feature that produced this body |
| `name` | str | display name (defaults to the feature id) |
| `kind` | str | `"brep"` (exact surfaces, STEP-able) or `"mesh"` (triangles) |

| Method | Returns | |
| --- | --- | --- |
| `faces(normal=, kind=, name=, on_plane=, above=, below=, inside=, nearest=, min_area=, max_area=, within=, tolerance=, allow_empty=)` | Selection | §4.8 |
| `face(name)` | Face | |
| `edges(direction=, kind=, name=, on_plane=, above=, below=, inside=, nearest=, of_face=, min_length=, max_length=, min_radius=, max_radius=, convex=, min_dihedral=, max_dihedral=, within=, tolerance=, allow_empty=)` | Selection | §4.8 |
| `edge(name)` | Edge | |
| `bbox()` | `((x,y,z), (x,y,z))` | |
| `translate(offset, name=, layer=)` | Body | |
| `rotate(angle, axis=, origin=, name=, layer=)` | Body | |
| `material(preset=, color=, roughness=, metallic=, texture=, scale=, opacity=, ior=, emissive=, transmission=, attenuation=, scatter=, scatter_g=, anisotropy=, edge_break=, depth=, label=, shadow=)` | Body (same) | appearance fields (see Material); `shadow=False` is guide geometry (§4.12) |
| `tessellation(tolerance=, angle=)` | Body (same) | this body's own chordal tolerance in mm and normal-turn bound in degrees, for everything that reads its triangles — export, render, `print_check`, `volume()`. Beats `export(..., tolerance=)` and the document's `--tolerance`; §6.1. A copy shares its source's, so set it on the source |
| `volume()`, `area()` | float | measured on the tessellation |
| `layer()` | str or None | the display layer's name, `None` on no layer. Read when called, so it follows a later `Layer.add` (§4.13) |
| `visible()` | bool | `False` once a boolean or a split consumed the body, or while its layer is declared hidden -- what a viewer shows and a whole-document export writes |
| `color()` | `"#RRGGBB"` | the material's base colour in sRGB, as `material(color=)` takes it; not a `paint(...)` colour |
| `section(plane, *, on=None, name=None)` | Sketch | the closed loops where `plane` crosses the body's tessellation, in the plane's own coordinates, nested as material and holes — a floor plan at a height *is* this, and `.area()` on it is the plan area at that level. `on=` lays the cut on another plane, coordinates unchanged (`mass.section(XY.offset(z), on=XY)`), which is what lets two cuts at different heights meet in `Sketch.intersect`. A plane that misses the body gives an empty sketch; the body is not consumed. Exact for a polyhedron, chords for a curved face. Feature `section`, args `plane`, `body`, `loops` |

`scale`, `mirror`, `fillet` and `chamfer` are globals (§4.4), not methods; a
body becomes a field with `field.from_body(body)` (§4.7).

`Body` values are equal when they refer to the same body id.

### Face, Edge

Immutable snapshots of a body's topology, taken when the query ran.

| Face attribute | Type | |
| --- | --- | --- |
| `name` | str | the generated topological name (§5) |
| `aliases` | list[str] | the other names that resolve to this face |
| `kind` | str | the analytic surface the face lies on, **recognised** by sampling and fitting to a micron: `plane`, `cylinder`, `cone`, `sphere`, `torus`. A free-form surface reports truck's own type instead: `bspline`, `nurbs`, `revolved`. So a loft's flat side is a `plane` and its band between two circles a `cone`, though truck holds both as NURBS; `faces(kind="nurbs")` still selects them, because the truck strings are aliases that match whatever the face was recognised as |
| `normal` | `(x,y,z)` | area-weighted average, unit; outward |
| `centroid` | `(x,y,z)` | area-weighted |
| `area` | float | mm² |
| `edges` | list[str] | bounding edge names |

| Edge attribute | Type | |
| --- | --- | --- |
| `name` | str | `faceA|faceB`, from the two faces it joins (§5) |
| `kind` | str | `line`, `circle`, `arc` (measured by a circle fit), `bspline`, `nurbs`, `intersection` |
| `start`, `end` | `(x,y,z)` | |
| `length` | float | mm (polyline length at the current tolerance) |
| `faces` | list[str] | the two adjacent faces |
| `radius` | float or None | the radius of the circle the edge fits — the same fit `frame(edge)` uses, on the polyline, so a rim a boolean left as a `bspline` still answers; `None` for an edge that is not an arc |
| `center` | `(x,y,z)` or None | that circle's centre; `None` with `radius` |

A Face remembers which body it came from, which is how
`fillet(part.faces(...).one().edges, 2)` needs no second argument.
Faces and edges are values, so they can be passed to `fillet(edges=...)`,
compared, sorted (`sorted(b.faces(), key=lambda f: f.area)`), and printed.

### Ref

What the user has selected in the UI, as seen by `selection()`.

| Attribute | Type | |
| --- | --- | --- |
| `kind` | str | `feature`, `body`, `face`, `edge`, `vertex`, `sketch`, `sketch_entity`, `param` |
| `feature` | str | feature id |
| `body` | str | body id or `""` |
| `name` | str | topological name or `""` |

### Material

A material is body state, not a value: `Body.material(...)` sets these
appearance fields on the body and returns it.

| Field | Type | Default | |
| --- | --- | --- | --- |
| `color` | `(r, g, b)` in 0..1 or `"#rrggbb"` | grey | linear RGB base colour |
| `roughness` | float 0..1 | 0.6 | 0 is mirror-smooth, 1 is chalk |
| `metallic` | float 0..1 | 0 | 1 tints the reflection with `color` and kills the diffuse |
| `texture` | str | `""` | `""`, `"checker"`, `"stripes"`, `"noise"`, `"wood"`, `"dots"` (albedo patterns); `"turned"`, `"milled"`, `"machined"` (machining marks, below) |
| `scale` | float | 10 | texture repeat length in mm; for the machining marks the groove pitch, 0.15 when the finish is named without one |
| `depth` | float mm | 0 | groove depth of the machining marks; 0 = a real tool's cusp (a twentieth of the pitch turned, 3 % milled) |
| `opacity` | float 0..1 | 1 | below 0.999 the body renders translucent |
| `ior` | float 1..2.5 | 1.5 | index of refraction; sets how strong the reflection is (Fresnel only, nothing is bent) |
| `emissive` | `(r, g, b)` or `"#rrggbb"` | black | linear radiance added after lighting; may exceed 1 |
| `preset` | str | — | a named material applied before the other keywords |
| `label` | str | — | a name for the material itself, not the body |
| `shadow` | bool | `True` | `False` makes the body **guide geometry**: a zoning envelope, a setback plane, a lot line, a section marker — the camera sees it, light does not. It casts no shadow, tints nothing and blocks no light in either renderer; its own surface is still lit and shadowed by the real bodies. Travels with the material through transforms, booleans and copies. `shadow_study()` and the sightline queries take explicit body lists, and a body passed to them is an obstacle whatever this says. See docs/views.md, "Guide geometry" |

The next five describe what a material does *inside* the solid rather than at
its surface, so only the path tracer (`cadpole render --backend path`) reads
them; the rasterizer and the viewport ignore all five. Every one defaults to
0, and 0 means "off", so a material written without them is unaffected.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `transmission` | float 0..1 | 0 | how much of the surface refracts rather than diffuses. This is what makes a part *clear* rather than half-transparent. 0 on a material whose `opacity` is below 1 means "use `1 - opacity`", so a script that only knows about `opacity` still refracts. |
| `attenuation` | float mm | 0 | Beer-Lambert distance: light that travels this far inside the solid comes out tinted by `color`. 0 = no absorption. This is the field that makes a thick section read darker and more saturated than a thin one, which is how a printed translucent part actually looks. |
| `scatter` | float mm | 0 | mean free path between scattering events inside the solid. A few mm gives a milky filament or a frosted resin; 0 is clear. |
| `scatter_g` | float -1..1 | 0 | how that scattering is aimed: 0 is even in all directions, positive is forward, negative is back. |
| `anisotropy` | float 0..1 | 0 | layer-line roughness: the highlight is stretched up the layer-stacking direction and narrowed along the extrusion, so an FDM wall reads as printed rather than injection moulded. Faces whose normal is Z show none of it, because a top surface has no layer lines. With a machining-mark texture the stretch follows the grooves instead, on every face. |
| `edge_break` | float mm | 0 | the path tracer shades every crease of the body as if deburred to a round of this radius (the shading normal turns toward the neighbouring face within it; no geometry changes), so edges catch the light as a machined part's do. 0 = the render's `--edge-break`, itself off by default. 0.1–0.3 is a real deburr. |

**A translucent body in each renderer.** Every renderer draws `opacity`
below 0.999, each as far as it can. The web viewer and a published page
blend the body at that opacity after the opaque ones and without writing
depth, so what is behind it shows and its own far wall shows through its
near one. The CPU rasterizer composites every translucent layer in front
of the nearest opaque surface, order-independently. The GPU backend keeps
only the nearest translucent layer. The path tracer refracts through
`transmission` (`1 - opacity` when that is 0), tints by `color` over
`attenuation` mm, and frosts what is seen through the body as `roughness`
rises: 0.05 reads clear, 0.2 already milky. `examples/enclosure.star`'s
shell is `opacity=0.42` for the first three and `transmission=0.92,
roughness=0.06, attenuation=26` for the tracer.

Out-of-range numbers are clamped rather than rejected. The albedo textures
are procedural and shared verbatim between the viewport, the CPU rasterizer
and the path tracer, so the same words mean the same look
everywhere.

**Machining marks** (`"turned"`, `"milled"`, `"machined"`) are relief,
not colour: a periodic tilt of the shading normal plus a roughness across
the grooves, and the anisotropic highlight follows the groove direction.
`"turned"` is fine concentric grooves about an axis — the face's own when it
is a cylinder, cone or torus, else the body's (the axis of its largest face
of revolution, else Z) — so an end face gets rings and a cylinder gets bands;
the profile is a round-nosed tool's sinusoid. `"milled"` is fine parallel
scallops along a face's ruling (the surface's `v` direction, which runs
between the sections of every loft and sweep; a fixed direction on a mesh
body), the profile a flank mill's run of arcs meeting at cusps. `"machined"`
is `turned` on the faces a lathe made — surfaces of revolution about the
body's axis and planes square to it — and `milled` on the rest, which is
what a billet part is in one line. Both are filtered by the pixel: grooves
smaller than a pixel average to flat and hand their slope variance to the
roughness, so a part reads the same at any resolution and shows no sparkle.
They exist in the path tracer and the CPU rasterizer (which roughens
isotropically); the viewport does not draw them.

```python
wheel.material(color="#f2f3f5", roughness=0.16, metallic=1.0, anisotropy=0.5, texture="machined")
knob.material(preset="aluminum", texture="turned", scale=0.2, edge_break=0.2)
```

#### Presets

`preset=` fills in every field at once; later keywords override it, so
`body.material(preset="glass", color="#204030")` is tinted glass. Unknown names are
an error that lists the valid ones. Every name is a member of the `preset`
namespace — `preset.glass` *is* `"glass"` — so `body.material(preset=preset.glass)`
is the same call, checked where it is written (§4.16).

A preset marked "path" below sets the print-material fields as well, so it
looks like every other grey solid under the rasterizer and only becomes
itself under `--backend path`.

| Preset | What it is |
| --- | --- |
| `pla_matte` | printed PLA, diffuse grey (the default look); layer lines in the highlight |
| `pla_silk` | silk PLA: the same plastic with a tight, strongly layered highlight |
| `pla_glow` | glow-in-the-dark PLA (path): emits a weak green, and lights what it stands next to |
| `petg_glossy` | glossy PETG, slightly stronger reflection (ior 1.57) |
| `petg_clear` | clear PETG (path): refracts, and tints over 55 mm of thickness |
| `petg_translucent` | translucent PETG (path): refracts, absorbs over 14 mm and scatters over 3 mm — milky, and darker where it is thick |
| `abs` | ABS, semi-matte |
| `tpu_rubber` | soft black rubber, almost no highlight |
| `resin_clear` | clear SLA resin (path): polished, refracts, absorbs over 45 mm |
| `resin_frosted` | frosted SLA resin (path): rough, absorbs over 25 mm and scatters over 6 mm |
| `glass` | clear glass, `opacity` 0.15 (path: refracts, barely absorbs) |
| `acrylic_clear` | clear acrylic, `opacity` 0.25 (path: refracts) |
| `acrylic_frosted` | frosted acrylic, `opacity` 0.6 and rough (path: refracts and scatters) |
| `brass` | metallic brass, lightly brushed |
| `aluminum` | metallic aluminium, lightly brushed |
| `steel` | metallic steel, lightly brushed |
| `wood_oak` | oak with the `wood` texture at a 40 mm repeat |
| `led` | a lit LED lens (emissive; path: an actual light source) |

#### Textures

Procedural: a pure function of the model's own millimetre coordinates. Moving
the camera never changes a pattern, and a body carries whatever part of the
pattern it occupies, so translating it moves it through the pattern. 2-D
patterns (`checker`, `stripes`) are projected triplanar — from all three axes,
blended by the surface normal — so they wrap a curved body without a seam or a
visible stretch. Each texture multiplies the base colour (it darkens, never
brightens) and may nudge the roughness. Sampling is filtered by the pixel
footprint, so a pattern smaller than a pixel fades to its average colour
instead of sparkling.

| Texture | Look | `scale` means |
| --- | --- | --- |
| `checker` | two-tone squares | square side |
| `stripes` | two-tone bands along one axis | band width |
| `noise` | fractal mottling; also breaks up the highlight | size of the largest blob |
| `wood` | growth rings around the Z axis, warped by noise | grain size (a ring every ⅔ of `scale`) |
| `dots` | polka dots on a 3-D lattice | dot spacing |

```python
pot.material(preset="pla_matte", texture="noise", scale=6)   # clay mottling
tray.material(preset="wood_oak", scale=25)                   # tighter grain
```

See `examples/materials.star` for a row of samples you can render.

### Part, Instance, Assembly, Frame

A `Part` is a reusable definition (bodies built once), an `Instance` a
placed copy of one (a pose; `.bodies`, `.face(name)`, `.edge(name)`,
`.body()`, `.frame()`, `.material(...)`), an `Assembly` a group of
instances (`.instances`, `.bodies`), and a `Frame` a coordinate system
(`.origin`, `.x`, `.y`, `.z`; `.offset(d)`, `.flip()`, `.turn(deg)`). See
§P4.

### Layer

What `layer(name, ...)` returns: a display layer the web UI's Layers panel
switches on and off and ghosts without re-evaluating, and `cadpole render
--layer` controls the same way. Metadata: nothing about a layer reaches
geometry; a whole-document export and `cadpole check` leave a hidden layer's
bodies out, and `bodies(layer=)` lists a layer's bodies. See
§4.13.

| Attribute | Type | |
| --- | --- | --- |
| `name` | str | unique per document |
| `doc` | str | the panel's tooltip |
| `visible` | bool | the layer's own default |
| `opacity` | float | 0..1, the layer's own default; below 1 its bodies are ghosted |
| `color` | `(r, g, b)` or None | a tint for the panel's swatch |

| Method | Returns | |
| --- | --- | --- |
| `add(*bodies)` | the same body, or a list of them | puts Body values, Instances and lists of them on this layer and hands them back |

`Layer` values are equal when they name the same layer.

### Field

An SDF expression, and a **value**: no id, not in the feature tree, not
exportable until `mesh()` turns it into a body. `field.bounds` gives the
conservative box the solid lives in, or `None` when the expression is
unbounded. See §4.7.

### Sun

Where the sun stands over a site at a date and a clock time, from `sun(...)`
(§B7): `.azimuth` (compass, 0 north, 90 east), `.elevation` (apparent,
refraction included), `.direction` (a unit vector toward the sun, `+y`
north, `+x` east, `+z` up), `.up`, `.sunrise`, `.sunset`, `.solar_noon`
(`"HH:MM"`, local clock), `.declination`, `.equation_of_time`, and what it
was made from (`.date`, `.time`, `.lat`, `.lon`, `.tz`, `.label`). A value,
not a feature.

### Shadow, ShadowStudy

What `shadow_study(...)` returns (§B7): a `ShadowStudy` is one `Shadow`
per sun (`.shadows`, `len()`, indexing, iteration, `.labels`, `.area()`,
`.max_area()`), and a `Shadow` is a footprint on the receiver: `.area()`
(mm²), `.loops` (in space), `.lit`, `.exact`, `.sun`, `.label`, `.receiver`,
`.casters`, and `.sketch(name=)`, which records it as a sketch on the
receiver's plane.

### Sightline, SightlineFan

What `sightline(...)` returns (§B8): `.clear`, `.blocked`, `.blocked_by`
(the body), `.blocked_face`, `.hit`, `.distance`, `.length`, `.from`, `.to`.
`sightlines(...)` and `view_cone(...)` return a `SightlineFan`: `.fraction`
(the rays that got through, 0..1; also `.visible`), `.blocked`, `.rays`,
`.visible_rays`, `.blocked_by` as `[(body, fraction), ...]`, `.target`.

---

## 4. Modelling: the shared core

Everything in this section is used by both kinds of work. A bracket and a
house are both sketches, extrusions, booleans and transforms; what differs
is what is built on top (`parts.md` for parts and assemblies, `buildings.md`
for buildings and plans).

Notation: `name(pos, pos, *, kw=default)`; `*` marks the start of
keyword-only arguments. "Feature" says what gets recorded in the tree.

### 4.1 Parameters

#### `param(name, default, *, doc=None, unit=None, min=None, max=None, step=None, choices=None)`

Declare a parameter and return its effective value.

| Arg | Type | Notes |
| --- | --- | --- |
| `name` | str | unique per document; the key used for overrides |
| `default` | int, float, bool, str, or `(x, y, z)` | its type fixes the parameter's type |
| `doc` | str | shown in the UI and to assistants |
| `unit` | str | display only (`"mm"`, `"deg"`, `"%"`). The value is a number *in that unit*: `param("climb", 10, unit="ft")` is ten feet and the script writes `ft(climb)`. Under the document's `units(...)` (below) the web UI also reads a length param — `unit` one of `mm`, `cm`, `m`, `in`, `ft`, `ft-in` — in that convention, `45'-6"` beside a `45.5 ft` input |
| `min`, `max` | number | numbers are clamped into range (with no diagnostic) |
| `step` | number | UI slider step; on a phone also what the stepper's − and + move by (without it, a round hundredth of the range) |
| `choices` | list[str] | for str params: the allowed values; `default` must be one |

Returns the override from the document (or from `--param`) if one exists and
has the same type, else `default`. An override of the wrong type is ignored
with a warning. Integer defaults return `int`.

The web UI's control follows the kind: a number is a slider (with
`min`/`max`) and a field, on a phone a full-width slider with a `− value +`
stepper; a bool is a switch; a str with `choices` is a segmented control when
they fit one line of a phone (at most four, none over twelve characters, 32
in all), else the platform's picker, never a text field; any other str is a
text field.

Feature: none (parameters live in `Model.params`). Diagnostics: `param "w"
declared twice`; `param "finish": default "matte" is not one of choices
[...]`.

```python
w = param("width", 40, unit="mm", min=10, max=200, doc="Outer width")
lid = param("lid", True, doc="Include the lid")
finish = param("finish", "matte", choices=["matte", "gloss"])
```

#### `units(unit, *, fraction=None)`

Declare how this document **writes** lengths: `units(units.ft_in)` makes
`cadpole inspect`, `distance()`'s report, `drawing()` and the `export_*` calls
(when given no `units=` of their own), the web UI's viewport, inspector and
length parameters read `45'-6"` rather than `13868.4`. A display
convention only: every number the model holds is a
millimetre, and `ft(45)` is still `45 * 304.8`; the kernel never reads it.
`units` is a callable vocabulary (§4.16): `units.mm`, `units.cm`, `units.m`,
`units.inch`, `units.ft`, `units.ft_in` are the strings the drawing parser
takes (`in` is a Starlark keyword and `ft-in` is not an identifier, so those
two carry the attribute spellings; the strings `"in"` and `"ft-in"` are
accepted too), and the same members feed `drawing(units=)` and
`export_svg/dxf/pdf(units=)`.

`fraction=` is the fraction of an inch an imperial dimension rounds to — 16
by default, 8 for framing, 0 for decimal inches; one of 0, 1, 2, 4, 8, 16,
32, 64 — and is ignored by the metric units. A clearance, a gap or a chord
error is always written as a decimal (`0.35"`), because a fit rounded to a
sixteenth is a different number. Areas read `1,240 sq ft` under the imperial
units and `115.2 m²` under the metric ones; volumes `cu ft` and `m³`.

Declared once per document (a second call is an error naming the first); a
library may not declare it — the document that loads
the library does. What it records: `Model.display_units {unit, fraction}`
and `InspectResponse.display_units`, absent for a document that declared
none, which every reader takes as millimetres.

```python
units(units.ft_in)                                # once, at the top
load("@cadpole/building.star", "ft", "inch")
mass = box(ft(30), ft(40), ft(10), name="mass")   # 9144 x 12192 x 3048 mm
print(distance(mass, other))                      # clearance(0.35" between …)
```

#### `length(text)`

Parse a length written the way a person writes it, to millimetres:
`length("12'-6\"")` is `3810`, and so are `length("12' 6\"")`,
`length("12ft 6in")` and `length("150in")`; `length("3.65 m")` is `3650`; a
bare number is millimetres. Fractions are taken (`"6 1/2\""`, or `"6-1/2\""`
and `"12'-6-1/2\""` as feet and inches are often written), `'`/`"` may be
written `ft`/`in`, and the result is snapped to the nanometre so `12'-6"` is
exactly 3810. The
plain multipliers `ft()`, `inch()`, `ft_in()`, `m()`, `cm()` live in
`@cadpole/building.star` (§B4).

### 4.2 Planes

#### `XY`, `XZ`, `YZ`

Constants; see §3.

#### `plane(*, origin=(0,0,0), normal=(0,0,1), x_dir=None)`

A plane from an origin and normal. `x_dir` is projected onto the plane; if
omitted, `X` is used unless the normal is parallel to X, then `Y`.
Diagnostics: `plane normal must be non-zero`, `plane x_dir must not be
parallel to normal`. Feature: none (planes are values).

### 4.3 Sketching

#### `sketch(plane=XY, *, on=None, origin=None, x_dir=None, name=None)`

Start a sketch. `plane` is positional and optional. `on=` is the alternative
to it (passing both is an error): a `Plane`, a `Face`, or a `Selection`
holding exactly one face. A face must be planar; its sketch plane has the
face's normal, its centroid as the origin and a canonical in-surface x
direction, so a parameter change moves the sketch but cannot spin it.
`origin=` (projected onto the plane) and `x_dir=` re-seat the frame of
whichever plane was chosen.

Feature: `sketch`, args `plane`, `loops`, `loopN` descriptions.

```python
profile = sketch(XZ, name="profile").polygon([(3, 0), (40, 0), (50, 80), (46, 80), (37, 4), (3, 4)])
```

#### `constrained_sketch(plane=XY, *, on=None, origin=None, x_dir=None, name=None)`

Start a sketch built from dimensions instead of coordinates; the plane
arguments are `sketch`'s. Returns a
`ConstrainedSketch` (§3): add named entities with `.point()`, `.line()`,
`.arc()` and `.circle()`, relate them with `.constrain(kind, ...)`, and extrude
or revolve the result. The solve happens on first use and is cached; §3 has the
constraint table, the diagnostics and the rules.

Feature: `sketch`, args `plane`, `constrained=true`, `entities`, `constraints`,
and after the solve `dof` and `loops`.

```python
s = constrained_sketch(XY, name="plate_profile")
a = s.point(0, 0, name="a", fix=True)
b = s.point(60, 0, name="b")
s.line(a, b, name="bottom")
s.constrain("horizontal", "bottom").constrain("distance", a, b, param("width", 60), name="width")
```

#### `curve(start, *, name=None)`

Start a `Curve` (§3) in world space at `start`, a point `(x, y, z)`. Build it
with `.line()`, `.line_by()`, `.arc(to, via=)`, `.bezier(to, controls=)` and
`.spline(points, tension=)`, then `.close()` it to make a `loft` section that
need not lie in any plane. Feature: `curve`, args `start`, `segments`,
`kinds` (the segments run-length encoded: `line, bezier(2) x47, arc, ...`)
and `closed`. `Path.on(plane)` records the same feature for a lifted path;
`Sketch.curves()` records nothing, the curves being the sketch's.

```python
# A blade section at the shroud, drawn from the middle of its trailing edge:
# pressure flank, round nose, suction flank — every point a world (x, y, z).
tip = (curve(te, name="tip")
       .line(press[-1])
       .spline([press[i] for i in range(n - 2, -1, -1)])
       .arc(suct[0], via=nose)
       .spline(suct[1:])
       .close())
```

#### The 2D toolkit

Offsets, booleans, corner fillets and chamfers, mirror and patterns on a
`Sketch`, each a global and a `Sketch` method that record the same feature
(as `translate(b, v)` and `b.translate(v)` do). Every operation returns a
new `Sketch` on the same plane, bound to a new feature whose input is the
source sketch, and names every loop of the result (§5): a loop that came
through untouched keeps its name, an offset loop is `<loop>/offset`, a
loop built from several is their names joined with `|` as an edge joins
its faces (`a/loop[0]|b/loop[0]`), a mirror image is `<loop>/mirror`, a
pattern copy `<loop>/copy[k]`, and two results that would share a name are
`#0`, `#1` in a canonical order. An unnamed hand-drawn loop is
`<sketch>/loop[i]` for this purpose, so a sketch named `s` offsets to
`s/loop[0]/offset`.

| Call | Method form | Feature |
| --- | --- | --- |
| `Sketch.offset(d, *, join=join.round, name=None)` | — | `offset2d`, args `distance`, `join` |
| `union2d(a, b, *, name=None)` | `a.union(b)` | `union2d`, inputs both sketches |
| `subtract2d(a, b, *, name=None)` | `a.subtract(b)` | `subtract2d` |
| `intersect2d(a, b, *, name=None)` | `a.intersect(b)` | `intersect2d` |
| `fillet2d(sketch, r, *, corners=None, name=None)` | `s.fillet2d(r, corners=)` | `fillet2d`, args `radius`, `corners` |
| `chamfer2d(sketch, d, *, corners=None, name=None)` | `s.chamfer2d(d, corners=)` | `chamfer2d`, args `distance`, `corners` |
| `mirror2d(sketch, *, axis=(0, 1), origin=(0, 0), keep=True, merge=True, name=None)` | `s.mirror2d(...)` | `mirror2d` |
| `pattern2d_linear(sketch, count, spacing, *, merge=False, name=None)` | `s.pattern2d_linear(...)` | `pattern2d_linear` |
| `pattern2d_circular(sketch, count, *, center=(0, 0), angle=360, merge=False, name=None)` | `s.pattern2d_circular(...)` | `pattern2d_circular` |
| — | `s.corners()` | none: a query, the corner names |

**What is exact.** Lines offset to lines and arcs to concentric arcs; a
boolean's vertices are closed-form line–line, line–arc and arc–arc
intersections; a fillet is an arc tangent to both segments. So an offset
circle extrudes to a cylinder, a filleted rectangle to four true cylinder
faces, and both export to STEP as such. A spline (Bézier) is the one
approximation: it is sampled to the document tolerance on the way in and
re-fitted to it on the way out. Consecutive segments of a result share
their vertex coordinates exactly, and a loop nothing touched comes back
as drawn, bit for bit.

**`offset(d, join=)`.** Positive `d` grows the material — outer loops
outward, holes inward — and negative shrinks it; both go by the loop's
nesting, so a hole's offset goes the right way whichever way it was drawn.
Where the offset opens a corner (a convex corner going out, a concave one
going in), `join` fills it: `join.round` (an arc of radius `|d|` about the
corner: the true offset, the default), `join.miter` (the two offset pieces
extended to where they meet, bevelled when that point is more than four
offset distances from the corner) or `join.bevel` (a line between the two
ends). Where the offset runs into itself the overlap is cut away: an
inward offset of a thin section pinches off cleanly at its trailing edge,
a loop that shrinks to nothing is dropped with an info note naming it (an
offset that empties the sketch says so too, and is not an error), and a
loop the offset cuts in two comes back as two (`…/offset#0`,
`…/offset#1`). A hole that grows into its wall takes the wall with it.
`join.miter` keeps a polygon a polygon, which is what a `loft` through
offset sections wants.

```python
foil = sketch(XY, name="foil").polygon(section(chord=100))
inner = foil.offset(-0.8, join=join.miter, name="inner")   # one loop, pinched off at the trailing edge
core = extrude(inner, 40, name="core")
skin = subtract(extrude(foil, 30, name="outer"), translate(core, (0, 0, -5)), name="skin")   # an exact skin
```

A section of more than about a hundred points puts the `subtract` over
the B-rep face budget of §4.5 and onto the mesh kernel with a note —
pass `kernel=kernel.brep` to insist, or draw the section coarser.

**Booleans.** Both sketches must lie on one plane (a coincident plane with
a different frame is mapped into the first's; a different plane is an
error). Material is where a sketch's loops, oriented by their nesting,
wind positively, so two overlapping outer loops in one sketch count as
covered. An empty result — `intersect2d` of sketches that do not overlap,
`subtract2d` of a cover — is an empty sketch with an info note, never an
error; `union2d` of sketches that do not touch is both sets of loops under
their own names; loops that share an edge are stitched into one and the
shared edge is gone; loops that touch at a single vertex stay two loops.

**`fillet2d` / `chamfer2d`.** A corner is a vertex where the tangent turns
(a rounded rectangle has none, a circle none); `Sketch.corners()` lists
them as `<loop>/corner[k]`, `k` the index of the segment that starts
there. `corners=` is a list of names (`"corner[2]"` picks that corner on
every loop; a longer suffix or the full name narrows it), of indices (on a
one-loop sketch) or of `(loop, corner)` pairs; leave it out for all of
them. The fillet's arc is tangent to both segments; the chamfer cuts back
`d` along each. Only corners between lines and arcs can be worked (a
corner on a spline is refused); a size that does not fit — the tangent
points would pass the far end of a segment, or two corners' fillets would
cross on the segment between them — is refused with the largest that
would fit. Exactly half a side from both ends is allowed, and the side
vanishes. The inserted arc or line takes its place in the loop's segment
order, so an extrude's `side[l.s]` names count it: fillet the sketch
before naming faces on it.

**`mirror2d` / `pattern2d_*`.** `mirror2d` reflects every loop across the
line through `origin` along `axis` (the sketch's y axis by default, so x
becomes -x, as `mirror(plane=YZ)` does for a body); `keep=True` keeps the
originals and `merge=True` unites them with the image, so a half drawn up
to the axis becomes one loop (`h/loop[0]|h/loop[0]/mirror`); `merge=False`
keeps both sets, `keep=False` is the image alone. `pattern2d_linear` and
`pattern2d_circular` mirror `pattern_linear` and `pattern_circular`: the
original plus `count - 1` copies, spaced by `spacing` or turned about
`center` over `angle`, with `merge=True` uniting copies that overlap (three
overlapping slots become one long slot); without it, overlapping copies
leave a sketch that will not extrude.

### 4.4 Solids

#### `extrude(sketch, distance, *, name=None)`

Extrude every material region of `sketch` by `distance` along the sketch
plane's normal. Negative `distance` extrudes the other way. `sketch` is a
`Sketch` or a `ConstrainedSketch`; a constrained one is solved at this call.

Returns one `Body` if the sketch has exactly one region, else a `list[Body]`
(one per region; bodies are named `name`, `name#1`, ...). Feature: `extrude`,
inputs `[sketch feature]`, outputs the bodies. Diagnostics: `extrude distance
must be non-zero`, `sketch has no closed regions to extrude`, `loop 2 is not
closed`.

Face names (§5): `start` (on the sketch plane), `end`, `side[l.s]`.

#### `revolve(sketch, *, axis=(0,0,1), origin=(0,0,0), angle=360, name=None)`

Revolve every region about the world axis through `origin` along `axis` by
`angle` degrees. `sketch` is a `Sketch` or a `ConstrainedSketch`, as for
`extrude`. A region that touches the axis makes degenerate faces; keep a small gap (e.g. a drainage hole) or use `sphere()`.
Returns Body or list like `extrude`. Face names: `seg[l.s]`, plus `start`,
`end` for partial revolves.

#### `loft(sections, *, ruled=True, match="index", cap="plane", name=None)`

A solid through two or more sections: the Gridfinity foot, a stacking-lip
socket, a wall-to-lip frustum, a lid cup — or a compressor blade between its
hub outline and its shroud outline. Returns a Body, or a list of Bodies if the
sections have several separate regions. The result is always an exact B-rep
body — a loft is not a boolean, so it never routes to the mesh kernel.

`sections` are, in any mix:

| Section | What it is | Rules |
| --- | --- | --- |
| a `Sketch` (or `ConstrainedSketch`) | one or more planar loops | with only sketches in the list: parallel, facing the same way, listed in order along their normal, the same loops and holes in every section |
| a closed `Curve` | one loop anywhere in space, planar or not | with any curve in the list, **list order is the order** — nothing is stacked along a normal — and every section is one loop: a sketch section is lifted onto its plane as drawn and must have a single loop (a hole has nowhere to go on a section that is one curve; loft the loops separately and subtract). Two sketches on planes that are *not* parallel loft as `a.curves()[0]` and `b.curves()[0]` |
| a point `(x, y, z)` | a tip, first or last | closes onto straight segments (a planar triangle) and circular arcs whose centre is on the tip's axis (a cone); a spline or an off-axis arc is refused |

`loft([sketch(XY).rect(10, 10), (0, 0, 12)])` is a pyramid.

**How sections are matched.** Loop `l` of every section is paired with loop `l`
of the next, and segment `s` with segment `s`, **in the order the sketches were
written** — never by a geometric search, because a search can change its mind
when a parameter moves and silently twist the solid, while construction order
is a pure function of the script. Two `rounded_rect`s of different sizes and
different corner radii therefore pair line to line and corner arc to corner
arc, and each corner band comes out as an exact cone, which is what a
45-degree chamfer of a filleted vertical edge is. Cadpole still runs the
geometric search, as a *warning*: if rotating a loop by `k` segments would have
fitted more than 1.5× better it says so and names the loop, which is what a
loop drawn from a different starting corner looks like.

**`match=` — when the segment counts differ.** Some sections cannot be drawn
with the same number of segments. A rim shell with magnet bosses lofts a plain 8-segment
wall ring up to a ring that detours in a U around every magnet boss.

| `match=` (a `match` member or its string, §4.16) | What it does |
| --- | --- |
| `"index"` (default) | Construction order only, as above. Unequal segment counts are a diagnostic. |
| `"arclen"` | Cut the sections' loops at the union of each other's vertices, measured as a fraction of the way round the loop, until the counts agree — then pair in construction order as usual. |

`"arclen"` splits **exactly**: a line piece is a collinear line, an arc piece is
a true arc of the *same circle*, a spline is subdivided by de Casteljau.
Nothing becomes a chord, so corner bands are still exact cones. It only ever
refines — sections that already agree are untouched, so turning it on cannot
change a loft that already built — and it never rotates a loop, so the seam is
still the one the script drew. Vertices that land within 0.5 mm of each other
are treated as one split point, which is what stops a sliver face; a piece that
would still be under a micron is a diagnostic naming the section and the loop.

It is off by default. Arc length is shared out over the whole loop, so a
detour that exists on one section and not the other spends a proportional
share of the other section's perimeter, which can drift the two sections'
corners a few millimetres apart.

```python
# A bossed rim shell: 8 segments up to 24, in 28 exact faces.
plain = sketch(XY).rounded_rect(39.6, 39.6, 6.3)
bosses = sketch(XY.offset(15)).add(boss_ring())     # a path() with U detours
brim = loft([plain, bosses], match="arclen", name="brim")
```

**Requirements.** Sketch sections must be parallel and facing
the same way, listed monotonically along the axis, with the same loops in the
same order, and the same nesting (which loop is a hole in which); curve
sections lift those rules and keep list order. Without `match="arclen"` every
section needs the same number of segments in each loop:
`rounded_rect(w, h, 0)` is a 4-segment rectangle, not an 8-segment rounded one,
so give a flat section a small radius if it has to pair with a rounded one.
Everything else is a diagnostic naming the section and the reason, never a
panic.

**`cap=` — how the two ends are closed.** One spelling for both ends,
or a 2-tuple `(start, end)`: `cap=("plane", "ruled")`.

| `cap=` (a `cap` member or its string, §4.16) | What it does |
| --- | --- |
| `"plane"` (default) | A flat face on the end section's plane. The section must be planar — a sketch always is, a curve is when its `plane` is not `None` — and a non-planar one is a diagnostic naming the section. |
| `"ruled"` | The loop **ruled against itself**: split at its seam (where the script started drawing it) and half-way round by arc length, one half reversed, and the same ruled band the sides get between the two — the loft's own band builder applied to one section. Exact for a blade tip, which physically *is* the ribbon between its two flanks. The split point is snapped to a vertex within 0.5 mm of half-way, else it becomes one; the two halves are then matched by arc length like two sections. Those vertices are planted on the capped section's wire only — the side faces next to it take the pieces as boundary edges and keep their surfaces — so no other section, band or name changes. |

The ribbon's two corners — the seam and the half-way point — are where a
ruling would have zero length, and a patch pinched to a point is a surface the
kernel cannot invert. So each
corner is closed by the two segments that meet there: two collinear lines
through it form the neighbouring band's straight ruling and need no face; two
segments that share a plane (two arcs of one circle, an arc and a line in its
plane, a flat corner of a spline) get a flat face bounded by them and the chord
between their far ends. A corner between two curved segments in different
planes is a diagnostic naming the section and the corner — start the loop on a
straight segment or at a flat corner. A blade section drawn from the middle of
its trailing edge, with a circular nose, satisfies both corners. Cadpole warns
when the ribbon would have been 3× narrower with the loop started somewhere
else, which is what a seam in the middle of a flank looks like: the seam is the
script's, so the warning says where it should have gone rather than
moving it.

Which way is *out* is read from the closed solid's own volume, so reversing the
list, or drawing every section the other way round, gives the same solid. A
section drawn the opposite way round from its neighbour is warned about (the
bands between them would cross), and `.reverse()` is the fix. The seam-rotation
warning runs on curve sections too, on their vertices in space.

**`ruled=` — straight sides, or one surface through every section.**

| `ruled=` | What it builds |
| --- | --- |
| `True` (default) | A straight ruled surface between each neighbouring pair of sections: a stack of bands, with a tangent crease at every section plane. A band between two concentric arcs of equal angular span is an *exact* cone, which is what makes a chamfer a loft. |
| `False` | One B-spline surface through **all** the sections at once — cubic in the section direction once there are four sections, quadratic at three — so the side is curvature-continuous across the sections in the middle and the whole column is a single face. |

`ruled=False` **interpolates**: the surface passes through every section
exactly (to about 1e-12 mm), so the sections are the shape you drew and not
control points to be approximated. Rational sections survive it — a skin
through concentric circles still cuts exact circles at every section — because
the compatibility and the interpolation are both done on homogeneous control
points. It is a global fit, so moving one section moves the surface either side
of it, which is what "smooth" means; a loft that must not move elsewhere wants
more sections, not a different algorithm. With two sections `ruled=False` is
the ruled loft, because there is nothing between them to be smooth about.

Two costs worth knowing. Interpolation is not shape-preserving: a cubic through
sections that step in size will overshoot a little between them, and the answer
to that is an intermediate section. And the sections are put on a common knot
vector, so sections whose segments are knotted differently multiply the control
points; Cadpole builds the surface exactly and does not remove knots
afterwards.

**A solid of revolution should be `revolve`, not a smooth loft.** A revolved
curve is exact, needs no sections at all, and tessellates as a strip where a
skinned stack of circles is a NURBS patch on a grid. Lofting is for sections
that genuinely differ.

A tip section closes onto straight segments (a planar triangle) and onto
circular arcs whose centre is on the tip's axis (a cone). A spline segment, or
an arc off that axis, is refused.

Face names (§5): `start` and `end` for the two end caps (a tip has
no cap on its side), `side[l.s]` for the face swept from segment `s` of loop
`l`, and — for a **ruled** loft through more than two sections — `side[l.s]#b`
for the band between section `b` and section `b + 1`, with `side[l.s]` staying
on as the group name for the whole column. Gridfinity's foot selects as
`foot/side[0.1]#2` for the 45-degree top chamfer at corner 1. A `ruled=False`
loft has no bands to distinguish — one face spans every section — so its names
are the plain `side[l.s]`. Under `match="arclen"`, `s` indexes the matched
segment list — the one both sections were cut into — not the segments of either
sketch on its own. With curve sections `b` is the script's list
index and nothing else, and a ruled cap is several faces, `start#k` / `end#k`
in order from the seam, with `start` / `end` as the alias for the whole cap.

```python
def ring(inset, z):
    return sketch(XY.offset(z)).rounded_rect(42 - 2 * inset, 42 - 2 * inset, 4 - inset)

foot = loft([ring(3.2, -5.0), ring(2.4, -4.2), ring(2.4, -2.4), ring(0, 0)], name="foot")

# A smooth hub: six circles, one surface, no visible bands.
hub = loft([sketch(XY.offset(z)).circle(r)
            for z, r in [(0, 8), (7, 11), (14, 17), (22, 24), (30, 28), (36, 30)]],
           ruled=False, name="hub")

# A compressor blade: its outline at the hub and at the shroud, two closed
# curves in space built from the same station list so that station i pairs
# with station i (the radial fibre), the root buried in the hub, both ends
# closed by ruling the loop against itself (examples/compressor_wheel.star).
blade = loft([root_outline, tip_outline], ruled=True, cap="ruled", name="blade")
```

#### `sweep_along(profile, path, *, frame="rmf", up=None, scale=1, twist=0, cap="plane", name=None)`

Sweep `profile` along `path`: the solid a planar section traces as it moves
along a curve in space — a pipe, a spring, a thread, a chamfer cutter along
an edge. `profile` is a `Sketch` (or `ConstrainedSketch`, or a closed planar
`Curve`), taken **as drawn**: wherever its plane is, that is where the sweep
starts, so a sketch on XY swept up +Z is the extrude, bit for bit. Draw it
at the path's start, usually on `path.start_plane`. `path` is a `Curve` —
`curve(...)` of lines, arcs, Béziers and splines, `helix(...)`, or a sketch
path lifted with `path.on(plane)` — open and tangent-continuous; a corner is
a diagnostic that says to round it with an arc (the way a bent pipe is
drawn), a closed path is refused. One body per region of the profile.

**What is exact, by segment kind.** A straight segment is a translation of
the profile (`tsweep`) and a circular arc a rotation about its own axis,
built as an exact NURBS surface of revolution, both exact B-rep — a circle
swept round an arc is a true torus that `faces(kind=kind.torus)` finds. A
spline or helix segment, and any segment with `twist=`, has no exact
surface on this kernel (a helix is not a rational curve): Cadpole runs one
cubic B-spline surface through copies of the profile placed along it (the
smooth loft's own skinning), chooses the stations by the bounds the
tessellator divides a patch by (the tangent turns by at most 8°
between stations, and the cubic interpolant's chordal bound is within the
target), then *measures* how far the surface sits from the true sweep and
refines until it is within a fifth of the document tolerance (0.01 mm at
the default). The measured figure is an info diagnostic on the feature and
its `approximation` arg. A long skinned segment is built in faces of at most 16 station
intervals, which carry one name in the grammar's `#k` pieces. A straight
segment with `scale=` stays exact (a ruled surface).

**`frame=`** — how the profile turns as it goes (the `sweep_frame` vocabulary,
§4.16):

| `frame=` | What it does |
| --- | --- |
| `"rmf"` (default) | rotation-minimising: the profile never turns about the path, so a square swept along any planar path is still the same square, the same way up, at the end. The initial normal does not change the geometry, only `end_plane` |
| `"frenet"` | the curvature normal. What a thread wants: on a helix it is the screw motion about the axis, so a profile drawn in the axial plane stays in it. Carried unchanged along a straight segment; refused at an inflection (the normal flips sides) or a joint where the curvature normal jumps, naming the place and `sweep_frame.rmf` |
| `"fixed"` | `up=` stays up, projected perpendicular to the tangent; refused where the path runs along `up` |

`up=` is `sweep_frame.fixed`'s direction and the seed of the others' normal.
`twist=` is an extra turn of the profile about the path over its whole
length, in degrees, and `scale=` the profile's size at the end relative to
the start (about the path point), both shared out by arc length. `cap=` is
`cap.plane`: a sweep's ends are copies of its planar profile.

Face names (§5): `start` and `end`; `side[l.s]` for the face
swept from segment `s` of loop `l` of the profile, and `side[l.s]#b` along
path segment `b` when the path has several, with `side[l.s]` the alias for
the whole run. Feature: `sweep`, args `frame`, `path`, `segments` (how each
was built), `length`, `approximation`.

```python
# A bent tube: an annulus along line–arc–line, every segment exact.
route = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))
tube = sweep_along(sketch(XY).circle(5).circle(4), route, name="tube")

# A spring: a round wire along a helix, drawn on the plane perpendicular to it.
coil = helix(10, 5, 6, name="coil")
spring = sweep_along(sketch(coil.start_plane).circle(1.5), coil, name="spring")
```

`@cadpole/mechanical.star` (§P6) builds springs, threads, bolts and nuts on
it.

#### `helix(radius, pitch, turns, *, hand="right", taper=0, start=(0, 0, 0), axis=(0, 0, 1), phase=0, name=None)`

A helix as a `Curve`: `turns` turns of `radius` about `axis` through
`start` (the centre of the first turn), advancing `pitch` mm per turn.
`hand.right` turns counter-clockwise about the axis while advancing along
it (a standard thread), `hand.left` the other way; `taper` is how much the
radius grows (or shrinks, negative) from the first turn to the last. The
curve starts at `start + radius` along the world axis least aligned with
`axis` (+X for a vertical helix), turned by `phase` degrees. Stored
exactly and swept from directly; approximated (within 0.001 mm, a cubic
Hermite B-spline) only where it must become an edge of a solid, such as a
loft section. Open, so `.line()`, `.arc()` and the rest continue it, and
`.start_plane` / `.end_plane` are the planes perpendicular to it at its
ends. Feature: `helix`, args `radius`, `pitch`, `turns`, `hand`, `taper`,
`start`, `axis`, `phase`, `length`.

#### Primitives: `box`, `cylinder`, `cone`, `sphere`, `torus`

| Call | Notes |
| --- | --- |
| `box(w, d, h, *, center=False, at=(0,0,0), name=None)` | size along x, y, z; corner at `at`, or with `center=True` centred in x/y on the plane z = `at.z` |
| `cylinder(r, h, *, d=None, height=None, at=(0,0,0), name=None)` | axis +Z, base centre at `at`; `d=` gives the diameter in place of `r`, `height=` is `h` by keyword |
| `sphere(r, *, center=(0,0,0), name=None)` | exact: two spherical caps meeting at the equator, each with its pole inside. Faces: the group `surface` — `surface#0`, `surface#1` — each `kind == "sphere"`. `at=` is accepted as a synonym of `center=` |
| `cone(r0, r1, h, *, at=, name=)` | axis +Z, base centre at `at`; `r1 = 0` makes a point. Built as a two-section `loft` of concentric circles, so the wall is an exact conical surface and behaves in a boolean. Faces: `bottom`, `top` (absent when `r1 = 0`), and the two-face group `side` |
| `torus(major, minor, *, center=(0,0,0), axis=(0,0,1), name=None)` | exact: the tube circle (radius `minor`) revolved about `axis` on a ring of radius `major`, drawn as an outer and an inner arc, so the faces are the groups `outer` and `inner` (three pieces each), every one `kind == "torus"`. `minor` must be smaller than `major`; `at=` is accepted as a synonym of `center=` |

These are sugar: they create no sketch feature, just one feature of kind
`box`/`cylinder`/... whose face names follow `extrude`/`revolve` (`box_1/end`
is the top). A sphere's and a torus's names are their own — `surface`;
`outer` and `inner` — with `revolve`'s `seg[0.s]` staying on as the alias.
Sizes must be positive.

#### `chamfer(target, distance, *, edges=None, d2=None, angle=None, kernel="brep", resolution=None, name=None)`

Cut the selected edges back and leave a flat: `distance` along each face,
`d2=` for a different setback along the second face, or `angle=` for the
angle in degrees at which the flat leaves the first face. "First" and
"second" are the two faces in the order the edge's *name* spells them
(`part/front|top` sets back along `part/front` first), so an asymmetric
chamfer reads the same way it is written.

The result is a **B-rep body**: each chamfer is a planar face on a straight
edge and a cone on a circular one, named `<feature>/face[<edge name>]`, with
every neighbour keeping its name. A run of edges on one line or one circle —
a rim is two arcs — is one chamfer under the first edge's name, the others
its aliases.

```python
part = box(30, 20, 10, center=True, name="part")
top = part.faces(normal=(0, 0, 1)).one()
out = chamfer(part, 1, edges=top.edges, name="out")     # four faces, mitred
out.faces(name="out/face[*]")                            # the four of them
chamfer(part, 1, edges="part/front|top", d2=2)           # 1 down the front, 2 across the top
chamfer(part, 2, edges="part/front|top", angle=30)       # 2 down the front, at 30 degrees
```

**How it is built.** One cutter solid per edge run; the cutters of a chain
are unioned into one tool and subtracted (or, for a concave chain, unioned)
in one operation. Corners mitre themselves: three cutters at a cube corner
leave three planes meeting at a point.

**What it refuses, and why.** The exact path never changes representation on
its own, so anything it cannot build is an error that names the edge
and the way out:

* an edge that **dies into a face** — the cutter would have to stop flush
  with it (a coplanar contact) or notch it. The message names the edge along
  that face, because chamfering both makes them mitre;
* a chain that **changes concavity** along its length;
* a **setback wider than a neighbour**, with the largest that does fit;
* a **smooth seam** (a cylinder's two halves): an error when named, skipped
  with a note when `edges=` was left out;
* an edge on a **free-form face**, whose dihedral varies along it;
* an edge that is **neither straight nor circular** — the cutter has no line
  to run along and no axis to turn about;
* a **straight concave edge**: the bead cannot overrun into air, nor stop
  flush. Concave chamfers are built around closed circular loops — a blind
  hole's floor, a pocket's — and into the profile otherwise;
* the **mouth of a hole** or the **root of a boss**. Their circle is an inner
  loop of the planar face, and the exact kernel will not split a face along a
  curve that encloses one of its inner loops — a plain counterbore fails the
  same way. Build the hole and its countersink as one tool before the boolean
  (a `cylinder` unioned with a `cone`, as `examples/gridfinity_baseplate.star`
  does), or put it in the profile;
* an **arc mitring a straight edge on a curved wall** (a flat on a shaft):
  the cone and the plane would meet the wall at a single point, a vertex of
  four faces the kernel builds only when all four are planes;
* a **circular rim of about 30 mm radius or more**: the booleans succeed but
  the solid they make cannot be meshed reliably, so the chamfer checks this
  itself and refuses rather than handing back a body that fails to mesh
  later.

**`kernel="field"`** takes the field path instead — the machinery of
`fillet(kernel="field")` pointed at a chamfer — and returns a **mesh** body
with the diagnostics described under `fillet`; `resolution=` belongs to that
path, and `d2=`/`angle=` do not.

**Cost.** Straight edges take milliseconds. A circular edge is a NURBS/NURBS
boolean, which is the slow class: a cylinder's rim takes on the order of
seconds.

#### `fillet(target, radius, *, edges=None, kernel="brep", resolution=None, name=None)`

Round the selected edges with a constant-radius blend. **Exact by default**:
the result is a B-rep body whose every blend is the natural quadric tangent
to both faces, and it exports to STEP as it is. `kernel="field"` is the mesh
path described at the end of this entry.

**Selecting edges.** `target` is the body; `edges=` restricts the call to part
of it and accepts a `Selection`, an `Edge`, a `Face` (its boundary edges), an
edge name, or a list of any of those. With no `edges=`, every edge of the body
is blended. A `Selection`, a `Face` and an `Edge` all remember which body they
came from, so the body argument is optional with any of them; a bare *name*
does not, so pass the body too. An `edges=` that selected nothing is an error:
rounding nothing is never what was meant.

```python
part = box(30, 20, 10, center=True, name="part")
rounded = fillet(part.faces(normal=(0, 0, 1)).one(), 2, name="rounded")   # a Face: its boundary edges

# Or by what the edges are, which survives a change to the part's size:
verticals = part.edges(direction=(0, 0, 1), convex=True)
rounded_sides = fillet(part, edges=verticals, radius=2, name="rounded_sides")

# Every edge of a box: twelve cylinders and a sphere octant at each corner.
rounded_all = fillet(part, 1.5, name="rounded_all")
```

**What it builds, and how.** Each blend is a cylinder along a straight
plane–plane edge and a torus round a circular one (a plane meeting a coaxial
cylinder or cone), named `<feature>/blend[<edge>]` with the neighbours
keeping their names; the new edges follow the face-pair grammar (§5). Two
routes, chosen per chain of edges:

* **A closed, tangent-continuous loop on one planar face** — a rim, the foot
  of a boss, the mouth or the floor of a hole, the rim of a rounded outline
  (straight runs and corner arcs alike) — is **re-wired directly**, with no
  boolean at all: each vertex of the loop is split into one point on the
  plane and one on the wall, the wall's seam is trimmed back to the contact
  circle, the loop's edges move out to the two contact curves and one blend
  patch per edge closes the gap. Milliseconds, and the cases the chamfer
  cannot cut (an inner loop, a rim of 30 mm or more) are exactly the ones
  this route takes.
* **Everything else** — a straight edge ending on perpendicular faces, a
  chain that turns convex corners, three blends at a corner — takes its
  topology from the chamfer with the setback `r·cot(γ/2)`, whose
  setback curves are the fillet's contact curves, then swaps every chamfer
  face for its blend over the same boundary: each end on a perpendicular
  planar face becomes the section arc, each mitre between two blends the
  planar **ellipse** in which their cylinders meet (exact as a rational
  quadratic), and where three blends meet at an orthogonal corner the three
  mitres and their vertex become three quarter arcs and a **sphere octant**,
  `<feature>/vertex[<vertex>]`.

**Refused, by name, with the fix.** Everything the chamfer's planner refuses
(an edge that dies into a face, a chain that changes concavity, a setback
wider than a neighbouring face, a free-form or free-curve edge, an arc
mitring a straight edge on a curved wall — each with the largest size that
fits where there is one), and the fillet's own: a corner that is not a right
angle with its third edge left sharp (the two blends' contact points on that
edge would not coincide, so no single mitre curve joins them); three blends
at a corner that is not orthogonal; a blend ending on an oblique face; a
radius deeper than the wall below a rim, with the largest that fits; a
feature — another edge of the body — inside the band the blend takes off the
plane or the wall, with the largest radius that keeps clear; a loop vertex
where anything but one straight seam arrives; a mesh body. Each names the
edge and the point, and ends with `kernel="field"` as the way out.

**`kernel="field"`: the mesh path.** The body is imported as a signed
distance field, a blend correction that is live only near the selected
edges is added to it, and the result is contoured back to triangles (§4.7).
So `rounded.kind == "mesh"`, and every call emits an info
diagnostic saying so together with the resolution it used, the surface
error it measured and the triangle count. `resolution=` belongs to this
path. It is the answer for what the exact path refuses — vertex blends
beyond the octant, unequal wedges, free-form pairs — at the cost of the
representation. The rest of this entry describes it.

**Accuracy.** On a pair of planar faces the blend itself is the *exact*
rolling-ball surface — the circular smooth-minimum is not an approximation
there — so the only error is the resampling. On a curved face the tangent
planes are read from the tessellation, which adds the tessellator's own
chordal error. As a guide, the radius of the meshed blend is within about
6 % at `resolution = radius / 4`, 4 % at `radius / 6` and under 1 % at
`radius / 12`.

`resolution` defaults to `radius / 6`, floored so a tiny blend on a large part
cannot ask for a grid nobody can afford. Pass it explicitly to trade time for
accuracy; the cost is in §4.7.1.

**Vertex blends.** Where three blends meet at a vertex the three
corrections combine with `max`, which is the intersection of the three blend
surfaces — a crease, not the spherical vertex patch a B-rep kernel builds.
Filleting every edge of a box therefore lands within 1 % of the analytic
rounded-box volume rather than within 0.1 %, and the corner is visibly a
corner at coarse resolutions.

**Diagnostics.**

* an edge whose two faces meet at less than 5 degrees is a tessellation seam,
  not a feature; it is skipped, and the warning names it;
* a radius larger than the material is thick is a warning naming the edge and
  the largest radius that does fit, measured by probing the body's own field
  where the rolling ball would have to sit. The verdict is a quantile over the
  edge, so a couple of awkward samples at a chain's end do not raise a false
  alarm;
* a selection that resolves to no usable edge, a non-positive radius, or an
  edge name the body does not have, is an error naming the problem.

#### `imprint(body, face, art, *, wrap=wrap.project, origin=None, up=None, name=None)`

Split one face of `body` along the loops of a sketch. The face keeps its
surface and its name and gets the loops as new inner boundaries; the pieces
inside them become faces of their own on the same surface; the body is
rebuilt around them. **No boolean runs and nothing is intersected** — the
loops *are* the new edges — so the split is exact wherever the loops lie: a
plane, a cylinder, a cone, a spline face. The result is a B-rep body. This
is the primitive behind `emboss`.

`face` is a `Face`, a `Selection` of one face, or a face name on `body`.
`art` is a `Sketch` or `ConstrainedSketch`, laid on the face by `wrap=`
(`wrap.project`, the default, or `wrap.unroll` on a cylinder or a cone —
see `wrap` below), placed at `origin=` with its `+y` along `up=` when
given and otherwise taken where it is in space; or a `Wrapped` from
`wrap()`, already placed (then `wrap=`, `origin=` and `up=` are refused —
they were `wrap()`'s).

| Face | Name | |
| --- | --- | --- |
| the parent face | unchanged (`plate/top`) | `kept`, with the loops as holes |
| a piece inside loop `L` | `<name>/imprint[L]` | the group `<name>/imprint` resolves to all of them |
| an island — a piece inside a hole of the sketch (the counter of a `B`) | `<name>/island[L]` | the group `<name>/island` |

`L` is the loop's own name when it has one — `label/glyph[3]/loop[0]` for
`text(..., name="label")`, so `split/imprint[label/glyph[3]/loop[0]]` is
one glyph — and its index otherwise. Several loops that overlap (kerned
glyphs) are united before the imprint, with a note, so no part of the face
is claimed twice; a sketch with a spline loop is taken as it is.

```python
plate = box(40, 30, 5, name="plate")
ring = sketch(XY.offset(5), name="ring").circle(6, center=(20, 15))
split = imprint(plate, "plate/top", ring, name="split")     # two faces on that plane, no boolean
paint(split, faces=split.faces(name="split/imprint"), color="#c8322f")
```

**Refused by name**, with the fix: a mesh body (no exact face to split); a
loop that leaves the face, crosses its boundary or crosses the surface's
seam (clip the sketch with `intersect2d`, or move it with `origin=`); a face
that closes through a pole of its surface (a `sphere()` hemisphere) or lies
on a surface of revolution with a curved profile (a sphere, a torus, a
revolved arc), which the tessellator cannot mesh with the loop in it.

#### `emboss(body, face, art, height=None, *, kind=emboss.raised, draft=0, wrap=wrap.project, origin=None, up=None, width=None, color=None, name=None)`

The imprint with a relief. `kind=` is the callable vocabulary `emboss`
(§4.16: `emboss.raised == "raised"`, and `emboss(...)` is this function):

| `kind` | What happens to the material | `height` |
| --- | --- | --- |
| `emboss.raised` (default) | the pieces stand proud of the face by `height`, along its outward normal | > 0 |
| `emboss.cut` | the pieces are sunk into the face by `height` | > 0 |
| `emboss.paint` | no geometry: the imprint, with the pieces painted `color=` | none |
| `emboss.scribe` | the sketch's outlines — closed loops and open paths — stroked `width=` wide (default `2·height`) with round ends, and cut `height` deep | > 0 |

`raise` is a reserved word in Starlark (Python's), the way `in` is, so the
member is `raised`; the string form `kind="raise"` is still accepted.

**A raise and a cut are one construction, never a boolean.** The piece's
loop is shared, edge for edge, between the parent face (as a hole), the
walls (as their base) and — through the walls' rungs — the top, so the
relief shares the face's boundary by construction. Only the sign of the height differs. On a plane the walls are
the extrude's planes and cylinders. On a cylinder or a cone the top is the
offset cylinder (radius `r ± height`) or the offset cone (the same
half-angle, the apex moved along the axis) trimmed by the offset loops, and
the walls are ruled between the loop on the surface and the same loop on the
offset surface — a plane for a projected line, a helicoid for a developed
one: exact surfaces of the kernel's own kinds. A word unrolled on a
cylinder therefore has arcs of exactly the sketch's widths, tops that
`faces(kind=kind.cylinder)` recognises at `r + height`, and planar walls on
its axial edges. The offset is along the face's *outward* normal, so
letters in a bore raise into the bore.

`draft=` (degrees, positive) narrows the relief away from the face —
mitred corner by corner, arcs kept concentric — for a cut as much as for a
raise; a spline loop, a corner too sharp to mitre, a loop that collapses or
crosses itself under the inset are refused by name. `color=` paints the
artwork faces (the tops, the floors, or the pieces for `paint`) with the
same `paint` feature `paint()` records; `kind=paint` requires it.

| Face | Name | |
| --- | --- | --- |
| the top of a raise / the floor of a cut inside loop `L` | `<name>/end[L]` | one spelling for both; the group `<name>/end` |
| a wall: loop `l`, segment `s` | `<name>/side[l.s]` | the group `<name>/side` |
| an island's plateau (inside a hole of the sketch) | `<name>/island[L]` | stays at the face's level |
| the parent face | unchanged | `kept` |

```python
plate = box(60, 30, 5, name="plate")
label = text("CADPOLE", size=8, plane=XY.offset(5), at=(12, 10), name="label")
tag = emboss(plate, "plate/top", label, 1, kind=emboss.raised, name="tag")  # exact; volume = plate + letters
tag.faces(name="tag/end")                                                 # the four tops
tag.faces(name="tag/end[label/glyph[2]/loop[0]]")                        # the top of the R

mug = cylinder(20, 40, name="mug")
w = wrap(text("MUG", size=7), mug.face("mug/side[0.0]"), method=wrap.unroll, name="placed")
mug = emboss(mug, "mug/side[0.0]", w, 1, kind=emboss.cut, color="#ff0000", name="engraved")

groove = emboss(plate, "plate/top", sketch(XY.offset(5)).add(path((5, 25)).line((35, 25))), 0.5,
                kind=emboss.scribe, width=1, name="groove")             # a stadium slot, 1 wide, 0.5 deep
```

**Refused by name.** Everything `imprint` refuses, and: a raise or a cut on
a sphere, a torus or a free-form face — no offset surface the exact kernel
represents (`paint` and `imprint` still work there); a cut deeper than a cylinder's radius;
a sketch wrapping most of the way round a cylinder or a cone (keep it within
one face); `width=` on anything but a scribe; a `Wrapped` from another body
or another face.

**Known limit.** The exact geometry is right, but on a cylinder the parent
face's tessellation around the new hole is coarse, so a mesh `volume()` of
the result reads about 0.5 % low on a 20 mm cylinder.

#### `wrap(sketch, face, *, body=None, method=wrap.project, origin=None, up=None, name=None)` → Wrapped

Lay a sketch on one face of a body and return it as a `Wrapped` (§3),
ready for `imprint` or `emboss`, or for its `.curves()`. `method=` is the
callable vocabulary `wrap`:

| `method` | The mapping | Exact for | Distortion |
| --- | --- | --- | --- |
| `wrap.project` (default) | every point moves along the face's normal *at the origin* until it meets the face | any face: the mapping is exact — a ray against a plane, a cylinder, a cone or a sphere in closed form, Newton on a spline face; a line projected onto a cylinder or a cone along a generator is a line, across it a circle; onto a sphere every line is a circle; the rest are fitted | foreshortening where the face turns away |
| `wrap.unroll` | the sketch is laid on the *developed* surface — a cylinder's `(s, t) ↦ (r cos(s/r), r sin(s/r), t)`, a cone's sector about the apex — and rolled back | a cylinder or a cone (a plane trivially): an isometry, so the sketch's millimetres are distances along the surface, the way a label wraps a can; a circumferential line is an arc, an axial one a line, a cone's line through the apex a generator, and every other segment is fitted (a diagonal on a cylinder is a helix, stored in closed form) | none |

`face` is a `Face` or a `Selection` of one face (both know their body), or
a face name with `body=`. `origin=` is a point on (or near) the face where
the sketch's own origin lands, with the sketch's `+y` along `up=` projected
onto the face; the sketch is then read in its own coordinates on the face.
Without it, `project` takes the sketch *where it is* — its outline projected
along the face's normal at the surface point nearest the sketch's origin,
an identity for a sketch drawn on a planar face — and needs the sketch's
plane parallel to the face there; `unroll` centres the sketch on the face
with `up` along the cylinder's axis or the cone's generator. `up` defaults
to the axis on a cylinder or a cone, and to world `+z` (or `+y` on a
horizontal face) elsewhere.

`unroll` is refused on a sphere or a torus ("does not develop onto a
plane").

#### `mirror(body, *, plane=YZ, keep=True, merge=True, name=None)`

Mirror `body` across `plane` (a `Plane` value: `XY`, `XZ`, `YZ`, an
`.offset()` of one, or `plane(origin=, normal=)`). The image is a solid the
right way out — a reflection turns a solid inside out unless every face is
inverted afterwards, which is what Cadpole does — and **every face keeps its
name**: `blk/top` is still `blk/top` in the image, so a selection that
worked before the mirror works after it (as for
`translate`). The body's surfaces keep their classes too: a mirrored
cylinder is `kind.cylinder` with the same radius.

The three forms:

* `keep=True, merge=True` (the defaults): **the body and its image fused
  into one**, the "model half, mirror it" flow. A half that ends *on* the
  plane meets its image face to face, and Cadpole stitches the two along that
  face instead of running a boolean — no internal face, still exact, the
  faces of both halves under their group names (`half/top#0`, `half/top#1`)
  and the face that lay in the plane gone. A half that crosses the plane
  overlaps its image and takes the ordinary `union` route, mesh fallback
  included. If the body and its image never touch there is nothing to fuse:
  the call returns a **list** of the two, original first, and an info
  diagnostic says so.
* `keep=True, merge=False`: always the list `[original, image]`; the
  original stays visible.
* `keep=False`: the image replaces the body, as `translate` moves one.

Mesh bodies mirror through the mesh path with the same rules. Feature:
`mirror`, args `plane`, `keep`, `merge`; a fused result that went to the
mesh kernel records `kind=mesh`.

```python
half = box(20, 10, 6, name="half")              # x from 0 to 20
whole = mirror(half, plane=YZ, name="whole")    # x from -20 to 20, one exact body
whole.faces(name="half/top")                    # both top pieces
left = mirror(arm, plane=XZ, keep=False)        # the image alone
```

#### `scale(body, factor, *, center=(0,0,0), name=None)`

Scale `body` about `center`: `factor` is one number for a uniform scale or
`(sx, sy, sz)` for one factor per axis. Factors must be positive — a
negative factor is a mirror, and `mirror()` is the call for that. Face
names are kept.

A uniform scale keeps every surface what it was (a cylinder wall is still
`kind.cylinder`, with the scaled radius). A non-uniform scale squashes a
cylinder into an elliptic one, which is no analytic surface Cadpole names, so
such a face reports `kind == "nurbs"` (or `"revolved"`) afterwards —
recognised from the scaled surface, never assumed from the old one — and
`faces(kind="cylinder")` no longer finds it. Planes stay planes. Feature:
`scale`, args `factor` (or `factors`) and `center`.

```python
big = scale(part, 2, name="big")
wide = scale(part, (1.5, 1, 1), center=(0, 0, 0), name="wide")
```

#### `pattern_linear(body, count, spacing, *, merge=False, kernel=None, name=None)`

`count` copies (the original is the first) offset by multiples of the
`spacing` vector. Returns `list[Body]`; the copies are features of kind `copy`
named `<name>_1`, `<name>_2`, ... when `name` is given. The original body is
not consumed.

#### `pattern_circular(body, count, *, axis=(0,0,1), origin=(0,0,0), angle=360, merge=False, kernel=None, name=None)`

`count` copies evenly spread over `angle` degrees (a full turn divides by
`count`; a partial arc divides by `count - 1` so the last copy lands on the
end angle).

**`merge=True`** fuses the copies into **one** Body (so the
call returns a Body, not a list) and consumes the source, recording one
`pattern_linear`/`pattern_circular` feature.

It **routes to the mesh kernel by default**, and says so in an info
diagnostic, because the copies of a pattern characteristically *touch*,
and a union whose operands merely touch, or share a face, is what the exact
B-rep kernel refuses. The result is then a mesh body — it keeps face
names, selection, STL and 3MF, and loses exact surfaces and STEP. `kernel=`
takes the same three values as `union` (§4.5): `"brep"` insists on the exact
kernel and turns a failure back into an error, `"mesh"` asks for the mesh
kernel without the diagnostic. A pattern of one instance is a copy, not a fuse,
and stays on the exact path.

Note what `merge=True` cannot do: bodies that meet along an **edge** only have
no manifold union, and no mesh can represent one. Overlap them, or carry them
past each other, so they share a face — `examples/gridfinity_bin.star` extends
each foot 0.5 mm above the plane the feet meet on, into material the body
already fills.

A non-merging `pattern_linear` or `pattern_circular` records one `copy`
feature per instance and no pattern feature. `pattern_along` and
`pattern_on_surface` (next) record one feature with N outputs,
named `<name>/copy[k]` and `<name>/row[i].col[j]`.

#### `pattern_along(body, curve, *, count=None, spacing=None, frame="rmf", up=None, start=None, end=None, align=True, merge=False, kernel=None, name=None)`

Copies of `body` along a `Curve` — a rivet line along a rib, posts round a
ring, beads on a helix. The body is placed **as drawn at the origin**: its
origin lands on each station and, with `align=True` (the default), its
**+z turns along the curve's tangent** and its +x along the frame's normal
(`up=`, else a stable direction off the start), so `cylinder(d=2, height=10)`
becomes a stud pointing along the path; `align=False` only moves it.
Stations are at **equal arc length**, in mm, through the curve's own length
tables: `count=` copies from the curve's start to its end (a closed curve
is divided like a full turn of `pattern_circular`, the last copy a step
short of the seam), or `spacing=` mm apart from the start, or both — that
many at that spacing, refused when the curve is too short, naming all three
numbers. `start=` and `end=` window the copies to that stretch of arc
length, in mm from the curve's start.

`frame=` is `sweep_along`'s `sweep_frame` vocabulary (§4.16): `rmf`, the
rotation-minimising frame (the default — no turning about the path),
`frenet` (the curvature normal: on a helix, the radial) or `fixed` (`up=`
stays up). A pattern reads the frame only at its stations, so — unlike a
sweep — the curve may be closed and may turn corners; at a corner the copy
faces the way the curve goes on. The curve is a `Curve` (`curve(...)`,
`helix(...)`, `path.on(plane)`, one of `sketch.curves()`); a sketch `Path`
has no plane and is refused with the lift named.

Returns the copies as a `list[Body]`: **instances** of the source under
**one** feature of kind `pattern_along`,
named `<name>/copy[k]`, each a rigid copy that shares the source's
tessellation (25 beads on a helix are one tessellation), and the
source is consumed (a tool drawn at the origin would otherwise stay there).
`merge=True` fuses them into one Body exactly as `pattern_linear(merge=True)`
does: one compose on the mesh kernel when the copies are apart, the
pairwise fold when they touch; `kernel=` as `union`'s. Copies whose bodies
overlap are a **warning** naming the first pair and how many more (a merge
adds that the fuse joins them). Feature args: `count`, `length`, `frame`,
`align`, `spacing`, `start`, `end`, `closed`, `path`, `up`.

```python
rail = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))
rivets = pattern_along(cylinder(d=2, height=3).translate((0, 0, -1.5)), rail, spacing=8, name="rivets")
beads = pattern_along(sphere(2), helix(10, 6, 3), count=24, align=False, name="beads")
posts = pattern_along(box(4, 4, 5), sketch(XY).circle(20).curves()[0], count=8, name="posts")   # where pattern_circular puts them, z along the tangent
```

#### `pattern_on_surface(body, face, *, rows=None, cols=None, count=None, spacing=None, normal=True, stagger=False, margin=0, merge=False, kernel=None, name=None)`

Copies of `body` spread over a face's surface — effusion holes on a conical
liner, knobs on a tyre's crown, studs on a plate. `face` is a `Face` or a
selection of faces that lie on **one surface**: truck makes a cylinder's, a
cone's and a sphere's side in two pieces and a revolve's faces in three, so
`can.faces(kind=kind.cylinder)` or `liner.faces(name="seg[0.1]*")` hands
the pieces over together and the seams between them are not boundaries;
faces on different surfaces are refused by name. The body is placed **as
drawn at the origin**: its origin lands on each surface point and, with
`normal=True` (the default), its **+z turns along the face's outward
normal** and its +x along the row, so `cylinder(d=3, height=5)` stands on
the face and a hole cutter is drawn straddling z = 0
(`cylinder(d=0.6, height=4).translate((0, 0, -2))`); `normal=False` only
moves it.

**Rows and columns are measured in millimetres on the surface**, never in
the parameter. A plane's rows run along its stable in-plane direction;
a cylinder's, a cone's and a torus's rows
are circles about the axis — *closed*: the copies go all the way round, the
first at the +X side — stacked along the axis, along the slant, or round
the tube; a sphere's are latitude circles about Z; a free-form (`nurbs`)
face's are its isolines, sampled by chord length. Three ways to count, and
they do not mix:

| Form | Means |
| --- | --- |
| `rows=, cols=` | the same number on every row (`rows=` alone is one column, `cols=` alone one row). On a cone the columns line up on the rulings and the pitch grows with the radius |
| `count=` (with `rows=` or not) | **that many in all, shared out among the rows in proportion to their length** (largest remainders), so the pitch is the same in mm on every row of a cone as nearly as whole numbers allow; without `rows=` the rows are chosen for near-square cells. `rows=10, count=600` is 600 holes |
| `spacing=du` or `(du, dv)` | as many as fit, `du` mm apart along a row and `dv` between rows, centred in the face; on a closed row the pitch is rounded so the ring closes |

An open row runs from `margin` to its length less `margin` with the copies
end to end (`n − 1` gaps, as `pattern_circular` divides a partial arc; a
row of one sits at the middle), and the rows stack across the same way.
`stagger=True` shifts every other row by half a pitch (an open row then has
one copy fewer: a brick course). `margin=` keeps every copy at least that
far from the face's boundary, and a copy that would fall outside the face's
**trimmed region** (a plate's hole, a cut slot) or nearer its boundary than
the margin is left out, counted in the feature's `skipped` arg and said in
an info diagnostic — both judged against the face's triangles within the
body's chord tolerance, so a row laid exactly at the margin is kept. A mesh
face with no surface is refused: it has no normal.

Returns the copies as a `list[Body]` of instances under one
`pattern_on_surface` feature, named `<name>/row[i].col[j]` (`j` counted
among the copies kept on row `i`), the source consumed; `merge=True` fuses
them into one Body as `pattern_along` does — 600 disjoint holes are one
compose, and `subtract(liner, holes)` one boolean
(`examples/effusion_liner.star`). The merged
body's faces keep the source's names with `#k` suffixes, as
`pattern_linear(merge=True)`'s do (`cylinder_1/side#37`), so a hole count
is `faces(kind=kind.cylinder).count()`. Overlapping copies are a warning,
judged as oriented boxes so that slender cutters leaning round a ring are
not called overlapping. Feature args: `face`, `surface` (`plane`,
`cylinder`, `cone`, `sphere`, `torus`, `nurbs`), `rows`, `per_row`
(`60 × 10`, or the list), `row_pitch` (one number, or `lo..hi`),
`across_pitch`, `normal`, `stagger`, `margin`, `skipped`, `closed_rows`,
and `cols`, `count` or `spacing` as given.

```python
hole = cylinder(d=0.6, height=4).translate((0, 0, -2))
holes = pattern_on_surface(hole, liner.faces(name="seg[0.1]*"), rows=10, count=600, margin=3, merge=True, name="holes")
liner = subtract(liner, holes, name="liner")                                    # 600 effusion holes, one boolean
studs = pattern_on_surface(cylinder(d=3, height=4), plate.face("top"), rows=3, cols=5, margin=6, stagger=True)
knobs = pattern_on_surface(sphere(1.5), can.faces(kind=kind.cylinder), spacing=(6, 8), name="knobs")
```

#### `hull(*bodies, name=None)`

The convex hull of one or more bodies (Bodies or lists of Bodies) as one
**mesh** body. Inputs are consumed, as `union`'s are; the result carries the
first body's material.

The hull is quickhull over every vertex of the operands' tessellations at
the document tolerance, so the hull of a sphere and a box is right — not
the hull of their corners — and accurate to the same chord error as every
other mesh-path result. Being a mesh body it keeps face names, selection,
STL and 3MF and has no exact surfaces or STEP; the call says so in an info
diagnostic, as `fillet` does. Coplanar triangles of the hull are one face,
so the hull of boxes has one face per flat side; every face is named
`hull`, and several are `hull#0`, `hull#1`, … under that group alias.
Feature: `hull`, args `bodies`, `kind=mesh`.

```python
pad = hull(sphere(4), sphere(4, center=(30, 0, 0)), name="pad")           # a capsule
foot = hull(box(20, 20, 2), cylinder(3, 15, at=(10, 10, 0)), name="foot")
```

### 4.5 Booleans

#### `union(*bodies, name=None, kernel="auto", weld=None)`, `subtract(*bodies, name=None, kernel="auto")`, `intersect(*bodies, name=None, kernel="auto")`

Two or more bodies, passed as separate arguments or as one list. `subtract` removes the second and later bodies from the
first. All inputs are consumed (hidden); the result takes the first input's
material. Feature kind is the operation name; args list the input body names,
plus `kernel=` when it was given and `kind=mesh` when the result is a mesh
body.

**A union never hands back a surface that is not a solid without saying so.
** Besides the exit gate every mesh boolean has (closed, no edge on
three faces, wound consistently, enclosing something), a union's
result is searched for triangles that pass through each other and faces
that lie on top of each other. When it has them -- the cleanup that collapses
slivers thinner than the tolerance is the usual cause -- the same result is
rebuilt without the cleanup and kept, with an info, if that is a solid;
otherwise the union is an error naming where:

```
error: main.star:12:5: mesh union produced a surface that is not a solid near
       (x, y, z): N pairs of triangles pass through each other (longest
       crossing … mm, deepest … mm); M pairs of coplanar triangles overlap …
       It is closed, but it passes through itself or doubles a face, so a
       slicer would have to guess at its inside … Overlap them by more where
       they meet; for axis-aligned boxes, union(..., weld=) joins them exactly
```

The search is skipped above 250,000 triangles, as `cadpole check`'s is.

##### Joining a model built out of boxes — `weld=`

`union(parts, weld=0.01)` joins bodies the way a printer fuses the parts of
one object, for a model drawn as hundreds of axis-aligned boxes -- the walls,
slabs, posts and treads of a house -- where a plain union stops at the first
two boxes that touch only along an edge:

- every operand that is an **axis-aligned box** is joined *exactly* on a grid
  (`cadpole_core::gridjoin`): each box is cut into the cells its own faces
  span, the solid cells' boundary is the answer, and every edge of it is
  checked to have two triangles wound against each other -- the result is
  closed, oriented and free of crossings by construction;
- **the weld**: on each axis, coordinates closer than `weld` are one, placed
  at the value most box faces use. Two boxes 0.004 apart then touch, a float
  spelling a coordinate two ways is one coordinate, and a box thinner than the
  weld in some direction closes to nothing -- a warning names the first one,
  so pick a weld under the thinnest part you mean to keep;
- **edge and corner contacts**, which no union can hold (the surface is
  pinched to nothing there), are bridged: a square prism `weld` across along
  every edge two boxes share with nothing beside it, a cube `weld` across at
  every corner two boxes share alone. That is what the printer does with them
  anyway; `print_check` sees the bridge as a thin wall, which is a true
  statement about the joint;
- every operand that is **not** an axis-aligned box (a cylinder, a sloped roof
  slab, a corbel, a rotated box) is then unioned onto the joined boxes
  exactly, in order. **The weld reaches it:** its vertex
  coordinates within `weld` of the boxes' planes move onto them first (when
  it is still a solid after), so a corbel modelled against a wall's face and
  a lip's underside meets them exactly, not a float rounding off. Where that exact tie still does
  not hold -- the mesh boolean sometimes leaves the wall's face whole under a
  face resting flat on it -- the faces resting flat on the boxes, with box
  material behind all of them, are pushed half a weld into the boxes and the
  union is tried again; then as modelled. An info says what was moved. If
  every try fails, the error names the body, says what was tried, gives the
  kernel's reason, the nearest of its coordinates the weld did not reach,
  and what to change: faces that rest on the boxes on their planes (within
  the weld) or well into them (0.05 mm or more), not a hair short or past;
  or the body built from boxes (a stepped corbel).

One body is enough (`union([b], weld=...)` returns its mesh). The result is a
`"mesh"` body named after the box sides its faces lie on (`slab/top`,
`wall_3/front#2`), with the bridges' faces named `<name>/weld`; it is on the
first operand's layer with its material. `weld` is a length in the model's
units and must be positive; `kernel="brep"` with it is refused. An info says
what was done:

```python
slab = box(30, 30, 1, at=(-5, -5, -1), name="slab", layer=walls)
a = box(10, 10, 10, name="a")
b = box(10, 10, 10, at=(10, 10, 0), name="b")            # meets a along an edge
c = box(4, 4, 4, at=(20, 20, 10), name="c")              # meets b at a corner
d = box(10, 0.5, 10, at=(0, -0.504, 0), name="d")        # 0.004 off a's front
glass = box(6, 0.002, 4, at=(2, 10, 3), name="glass")    # thinner than the weld
story = union(slab, a, b, c, d, glass, weld=0.01, name="story")
```

```
info: main.star:8:9: union with weld=0.01: 6 boxes joined exactly on a grid of
      10 x 11 x 6 lines; 2 coordinates welded onto a neighbour; 1 contact along
      an edge and 1 at a corner bridged 0.01 wide, the first near (10.0000,
      10.0000, 0.0000)
warning: main.star:8:9: union with weld=0.01: 1 box thinner than the weld in
      some direction closed to nothing (the first is glass); a smaller weld
      keeps it
```

##### Which kernel runs — `kernel=`

Cadpole has two geometry kernels (docs/design.md): the exact B-rep kernel (truck),
which keeps surfaces, edge names and the road to STEP, and the mesh kernel
(manifold), which is topologically robust and whose cost follows the region
the two bodies actually share rather than their total size.

| `kernel=` (a `kernel` member or its string, §4.16) | What happens |
| --- | --- |
| `"auto"` (default) | B-rep first, mesh kernel when the B-rep kernel fails or the pair is over budget. The result is a `"brep"` body when truck did the work and a `"mesh"` body otherwise. |
| `"brep"` | Exact kernel only. A failure is an error, never a demotion — this is what a part that must export STEP asks for. |
| `"mesh"` | Mesh kernel only. B-rep operands are tessellated first, each at its own `tessellation(tolerance=)` when it has one and at the document's mesh tolerance otherwise, so the result is approximate to that; the result is always a `"mesh"` body. |

The `"auto"` rule, decided *before* each pairwise call because the B-rep
kernel cannot be interrupted:

1. either operand is already a mesh body → mesh kernel;
2. `faces(a) + faces(b)` over the **face budget** (200) → mesh kernel;
3. otherwise truck; if it fails, the same pair is retried on the mesh kernel.

Nothing about the route depends on how long anything took: the same script
gets the same kernels on every run, on every machine and in the browser, so
a body's `kind` — and whether it can export STEP — is a property of the
document. A boolean that is slow on the exact kernel stays slow on
every edit; the way to make it fast is `kernel="mesh"` on that call.

Every route that is not plain B-rep emits an **info** diagnostic saying which
rule fired and the face count, for example:

```
info: main.star:190:7: union of 321 faces is over the B-rep budget of 200:
      routed to the mesh kernel, whose cost follows the touching region
      rather than the whole body
```

A mesh body keeps working with `faces()`, `edges()`, `bbox()`, selection,
materials, rendering and STL/OBJ export, and its faces keep the names they had
before the boolean (manifold propagates per-triangle provenance). What it
cannot do is go back: there is no mesh → B-rep conversion, so STEP export and
exact surface queries need `kernel="brep"` from the start.

**Touching operands on the mesh kernel.** Manifold resolves a contact
only when the coordinates are *exactly* equal, so before every mesh boolean
the planes the two operands share are snapped onto one plane: a face standing
on a face, a pocket flush with a floor, a box level with a box. The second
operand's vertices move onto the first operand's plane (the first never
moves), the band is the weld distance — 0.1 µm, or a millionth of the pair's
size — so it repairs two spellings of one plane and never moves a feature, and
a gap wider than that is left where it is and named. When something moved the
feature carries an info:

```
info: main.star:12:5: mesh union: snapped 1 face group (2 faces: `a/top`,
      `b/bottom`) onto 1 shared plane, 12 vertices moved, the furthest by
      0.04 µm — within the 0.10 µm band, so two spellings of one plane become
      one and manifold's exact engine resolves the contact as a tie instead
      of a sliver or a gap
```

A contact the snap cannot make exact — a boss whose side is tangent to a
hole, an edge or a corner resting in a face — is named when the result shows
it (two bodies that did not merge, a surface that is not a solid, or a
refusal), with the fix: `mesh union: tangent cylinders, one inside the other:
`plate/back#0` and `boss/back` touch along a line (axes 2.000 mm apart, the
difference of the radii 4.000 and 2.000; 0.00 µm from exact tangency, axes
0.0 µrad apart). Give `boss/back` at least 0.100 mm of interference (radius
2.100 instead of 2.000) …`. Curved tangency is not resolved: the union comes
back as two bodies that meet without merging, and the info says so.

### Contact: bodies that touch rather than cross

`truck` computes *transversal* booleans: it intersects the two boundaries and
cuts every face along the intersection **curves**. Two faces that are coplanar
hand it a 2-dimensional intersection instead, which it has no way to cut along,
so it fails. Every B-rep boolean therefore runs a cheap pre-pass first, and a
pair whose faces coincide gets one of three contact operations
instead of the kernel:

1. **`glue`** — an additive pair that *touches*, faces back to back, and shares
   no volume. The touching faces are merged and the two shells become one: no
   boolean, no recomputed surface, and no tolerance anywhere in the answer.
   `union` routes to it by itself, so a script never has to know; calling
   [`glue`](#glue) by name buys the refusal instead of a quiet kernel boolean.
2. **Cutter over-extension** — a `subtract` or `intersect` whose *tool* has a
   face coplanar with the body's. The tool is stretched along that face's own
   normal by four tolerances, which is the `depth + 0.02` a CAD user writes by
   hand — but only when the material the tool gains is **proved** to miss the
   body, so the answer cannot change. This is what makes a pocket whose floor
   is the body's own floor, a through slot flush at both ends, and a cutter
   flush with a wall all work. Additive geometry is never moved.
3. **A diagnostic** for everything else, naming the two faces, both boxes, the
   overlap on each axis and the way out:

```
error: main.star:275:6: kernel operation failed: intersect: coplanar faces,
       flush — the first body's face #12 (plane, box (0.000, 0.000, 0.000)..
       (0.000, 125.500, 40.800)) and the second body's face #4 (plane, box
       (0.000, 0.000, -5.500)..(0.000, 125.500, 41.300)) lie in the same plane
       (normal (-1.000, 0.000, 0.000), facing the same way, sharing
       5115.400 mm²); they overlap by X 0.000, Y 125.500, Z 40.800 mm. ...
       growing the tool past that face would change the answer: the body's
       own surface passes through the 0.400 mm the tool would gain, near
       (-0.125, 20.750, -0.125). That is a partial coincidence — the body both
       ends on that plane and crosses it — and Cadpole refuses it rather
       than guess. Move the tool's face off the body's own plane by at least
       0.5 mm, make the cut before the material that crosses the plane is
       added, or pass kernel="mesh". By name: faces `body/side[0.3]` and
       `trim/left` are coplanar within 0.00 µm (normals 0.0 µrad apart),
       facing the same way: the faces are flush over 5115.400 mm². Move the
       tool's face `trim/left` off the body's plane by at least 0.5 mm …
```

Every refusal opens with the **case** — `coplanar faces, touching` or
`flush`, `coincident surfaces`, `tangent cylinders, side by side` / `one
inside the other`, `tangent a cylinder on a plane`, `tangent a sphere on a
plane`, `edge-on-face`, `vertex-on-face`, `edge-on-edge`, `corner-on-edge`,
`near miss` — then the two faces, the measured distance and angle, and the
fix with its number. The exact kernel labels faces by index, kind and
box because a bare solid has no names; under `kernel="auto"` the routing info
and under `"brep"` the error then add the same contact **by name** from the
feature's own vocabulary, as above.

The cases that are still refused, and why each is a refusal rather than an
operation:

- **A flush additive pair** — two bodies level with each other on a shared
  plane, both on the same side of it. The union's face there is the two faces
  merged, which is a boolean *in* the plane.
- **A partial coincidence** — the body both ends on the plane and crosses it,
  as the Gridfinity bin's footprint trim does. Growing the tool leaves the
  crossing material standing proud by the size of the growth; shrinking it eats
  the body.
- **A tangent pair** — two boundaries that graze rather than cross, or an edge
  of one lying in a face of the other. Found after the kernel fails, since
  finding it beforehand is the work the kernel exists to do, and then named
  by case with the numbers: `tangent cylinders, one inside the other: … touch
  along a line (axes 2.000 mm apart, the difference of the radii 4.000 and
  2.000; 0.00 µm from exact tangency, axes 0.0 µrad apart) … Give the second
  body's face #1 … at least 0.200 mm of interference (radius 2.200 instead of
  2.000)`. Two Gridfinity feet one pitch apart meet the floor they stand on
  along a line exactly like this.
- **A feature finer than the boolean tolerance** — a 0.1 mm pocket floor at a
  0.1 mm tolerance is the body's own floor as far as the kernel can tell, and
  the over-cut that would clear the coincidence would cut the floor away. The
  refusal names the thickness it found; ask for a finer `--boolean-tolerance`
  and it resolves.

A boolean on `kernel="mesh"` refuses one thing too, for the same reason the
exact kernel refuses the contacts above: a result that is **not a solid**. Two
boxes meeting along one edge unite into a surface pinched to nothing along it,
and `intersect` of two boxes sharing one face is a zero-area sheet — neither
has an inside, so neither is a body. Overlap the operands by 0.5 mm where they
touch, or keep them as separate bodies. The mesh path says which contact it
saw (`mesh union: edge-on-face contact: an edge of `b/front` lies in `a/top`
within 0.00 µm …`), so the refusal has a cause on the same feature.

### Kernel limits

- **Bodies that never touch are answered without the kernel.** `subtract(a, b)`
  where `b` is nowhere near `a` is `a`; `intersect` of the two is an error,
  because the result is empty and Cadpole has no empty body; `union` of the two is
  an error on the exact kernel, because one B-rep solid is an outer shell plus
  its voids and two separate lumps are not that — on `"auto"` the mesh kernel
  takes it and returns one mesh body with two lumps. Both messages name the gap
  between the bounding boxes and what to do about it.
- **A body fully inside another needs no kernel either.** `subtract(outer,
  inner)` where `inner` is entirely surrounded by material is a solid with a
  **void** — one body, two shells, and the volume goes down. `intersect` is the
  inner body and `union` is the outer one.
- **A boolean *on* a body that already has a cavity is refused** on the exact
  kernel and routed to the mesh kernel by `"auto"`: truck's boolean folds every
  shell into one lump. Cut first, hollow last, or pass `kernel="mesh"`.
- **A tool that only rests against the body** removes nothing: `subtract`
  returns the body unchanged and `intersect` is an error, because the result is
  a surface rather than a solid.

```python
part = subtract(plate, hole_left, hole_right, name="part")
bin = union(hollow, feet, name="bin")                  # auto: mesh, and says so
lid = union(shell, rim, name="lid", kernel="brep")     # must stay exact
rough = union(a, b, name="rough", kernel="mesh")       # never mind the surfaces
```

#### `glue(*bodies, name=None)` {#glue}

Join bodies that **touch** on coincident faces, by merging those faces. Inputs
are consumed. Always the exact B-rep kernel, so there is no `kernel=`.

This is `union` for parts that meet flush rather than overlapping: Gridfinity
feet standing on a bin's floor, a lip on a wall top, a tab flush with a wall.
It is a topological merge — the touching faces are dropped, the two shells are
stitched into one — so nothing is intersected, no surface is recomputed, and
the result is exact where a kernel boolean is not even defined.

`union` already routes a touching pair here on its own, so you never *have* to
call `glue`. What calling it buys is the **refusal**: it is an error if the
bodies overlap, or touch only along an edge, or their faces coincide only in
part, where `union` would quietly run a kernel boolean instead. Say `glue` when
two parts are meant to meet flush and you want to hear about it if they stop.

The two shapes a touching pair may take are the two it accepts: the faces have
the **same boundary** (both are dropped), or one lies **strictly inside** the
other (the small one is dropped and its outline becomes a hole in the large
one). A patch that only partly covers the other is refused — merging those two
means a boolean in the plane itself.

```python
col = pattern_linear(foot, 3, (42, 0, 0), merge=True, kernel="brep")  # glue
lid = glue(shell, rim, name="lid")     # an error the day the rim stops touching
```

### 4.6 Transforms and copies

`translate` and `rotate` exist both as globals and as `Body` methods; they
are the same features. Use whichever reads better: `translate(b, (10, 0, 0))`
in pipelines, `b.translate((10, 0, 0))` when chaining. `mirror` and `scale`
are in §4.4.

| Call | Notes |
| --- | --- |
| `translate(body, offset, *, name=None)` | consumes `body` |
| `rotate(body, angle, *, axis=(0,0,1), origin=(0,0,0), name=None)` | degrees; right-hand rule |
| `copy(body, *, offset=(0,0,0), name=None)` | original stays visible |

Names survive transforms unchanged (§5): `lid/end` is still `lid/end` after
`translate`.

For several placed copies of one body — four wheels, three planets — see
`part` / `instance` in §P4: an instance shares the part's tessellation
and is placed by frames, where `copy` bakes a solid per step.

### 4.7 Organic and field bodies

Signed-distance-field modelling for planters, vases, blended forms — and, via
§4.4's `fillet`/`chamfer`, for rounding the edges of ordinary bodies. A
`Field` is an expression **value**: it has no id, is not in the feature tree,
and nothing is computed until it is meshed.

```python
f = field.cylinder(40, 90)
f = f.smooth_subtract(field.cylinder(37, 90).translate((0, 0, 4)), 2)   # hollow, rounded floor edge
f = f.displace(field.noise(scale=12, amplitude=1.2, seed=3))            # texture the surface
f = f.twist(0.8)                                                        # degrees per mm about Z
vase = mesh(f, resolution=0.4, name="vase")                             # a mesh body
```

| Constructor | |
| --- | --- |
| `field.sphere(r)` | centred on the origin |
| `field.box(w, d, h, *, round=0)` | centred; `round` rounds all twelve edges |
| `field.cylinder(r, h, *, round=0)` | Z axis, centred; `round` rounds both rims |
| `field.cone(r1, r2, h)` | capped; `r1` at the bottom, either may be 0 |
| `field.torus(major, minor)` | in the XY plane |
| `field.capsule(a, b, r)` | |
| `field.half_space(*, origin=(0,0,0), normal=(0,0,1))` | unbounded on its own |
| `field.noise(*, scale=10, amplitude=1, seed=0, octaves=1)` | deterministic value noise; the same seed gives the same field on every machine |
| `field.from_body(body)` | signed distance to the body's triangles (BVH + angle-weighted pseudo-normals); a B-rep body is tessellated at the document tolerance first. The body must be a solid; anything else is an error naming the defect (`import_mesh(..., repair="weld")` fixes welds and inside-out shells) |

| Method (returns `Field`) | |
| --- | --- |
| `union(other)`, `intersect(other)`, `subtract(other)` | exact |
| `smooth_union(other, k)`, `smooth_intersect`, `smooth_subtract` | `k` = blend radius in mm. On a **planar** face pair this is the exact rolling-ball fillet, not an approximation of one |
| `chamfer_union(other, d)`, `chamfer_intersect`, `chamfer_subtract` | `d` = setback in mm along each face |
| `offset(d)`, `shell(t)`, `invert()` | grow/shrink; wall of thickness `t` centred on the surface |
| `translate(v)`, `rotate(angle, *, axis=(0,0,1))`, `scale(s)`, `mirror(*, normal=(1,0,0))` | `scale` is uniform only: a non-uniform scale is not a distance field, so the mesher's bounds would stop being sound |
| `twist(degrees_per_mm)` | about Z |
| `taper(z0, z1, *, bottom=1, top=1)` | xy scaled linearly with z, held constant outside `[z0, z1]` |
| `displace(source, *, strength=1)` | push the surface along its own normal |
| `mesh(*, resolution, name=None, simplify=True)` | returns a `Body`; same as the `mesh()` global |

`Field.bounds` is `((min_x, min_y, min_z), (max_x, max_y, max_z))`, or `None`
for an unbounded expression (a bare `half_space`). Meshing an unbounded field
is a diagnostic that tells you to intersect it with something finite.

A `Field` is a value, `mesh()` turns it into a mesh body, and mesh bodies
boolean against B-rep bodies through the mesh kernel. A meshed field is
watertight by construction, so it goes into `union`/`subtract`/`intersect`
with no further ceremony. Selection on a meshed field is by face group.

#### `mesh(field, *, resolution, name=None, simplify=True)`

The **only** conversion from a field to geometry. `resolution` is required
and is the cell size in millimetres at the finest level of the contouring
octree. `simplify` lets flat regions collapse to fewer triangles: it sets the
contouring error threshold to a quarter of a cell.

Meshing is **manifold dual contouring** (`cadpole-field::mdc`): an octree
descended with interval arithmetic so only the surface band is ever sampled;
one QEF vertex per connected component of the surface inside a cell, clamped
into that cell, so sharp edges and corners survive and the dual surface does
not fold through itself; quads dual to every cut grid edge. The output is
watertight and 2-manifold — both are asserted, not hoped for — and the
`mesh()` call refuses to return anything that is not.

It is deterministic: the same script gives byte-identical triangles on every
run, and no RNG is involved anywhere (the noise field is an integer hash of
the lattice point).

### 4.7.1 What a field costs

Cost follows the *surface area divided by the cell area*, not the volume:
halving `resolution` roughly quadruples the triangles and the time.

### 4.8 Queries and selectors

#### `Body.faces(...)` / `Body.edges(...)` → `Selection`

Every filter given must hold. With no filters, every face (or edge).

| Keyword | On | Means |
| --- | --- | --- |
| `normal=(x,y,z)` | faces | the outward normal is within `within` degrees. **Signed**: `(0,0,1)` is the top, not the bottom |
| `direction=(x,y,z)` | edges | a **straight** edge's tangent is within `within` degrees. **Unsigned**, because an edge has no preferred direction; a curved edge never matches |
| `kind=` | both | a `kind` member or its string (§4.16): surface class `kind.plane` (`"plane"`, alias `"planar"`), `cylinder`, `cone`, `sphere`, `torus`, `bspline`, `nurbs`, `revolved`; curve class `line`, `bspline`, `nurbs`, `intersection`, `circle`, `arc` |
| `name=` | both | `*`/`?` glob over the generated name, an alias, or the local part — `name="side[*]"` needs no feature id |
| `on_plane=plane` | both | the **whole** entity lies on the plane, within `tolerance` mm |
| `above=plane` / `below=plane` | both | the **whole** entity is on that side of it |
| `inside=(lo, hi)` | both | the whole entity is inside the box. This is `bbox()`'s shape, so `a.faces(inside=b.bbox())` reads well |
| `nearest=(x,y,z)` | both | reduce to the single closest by centroid |
| `of_face=face_or_name` | edges | the edges that bound that face |
| `min_area=` / `max_area=` | faces | mm² |
| `min_length=` / `max_length=` | edges | mm |
| `min_radius=` / `max_radius=` | edges | mm, for arcs and circles |
| `convex=True\|False` | edges | material on the inside of the fold |
| `min_dihedral=` / `max_dihedral=` | edges | degrees: 0 is a tangent seam, 90 a box corner |
| `within=` | both | angular tolerance in **degrees** (default 5) |
| `tolerance=` | both | length tolerance in **millimetres** (default 0.001) |
| `allow_empty=True` | both | do not warn when nothing matches |

`within=` is degrees and `tolerance=` is millimetres, here and everywhere.
Passing `tolerance=` for an angle is an error naming `within=`, not a
number reinterpreted as a length.

A plane or half-space test takes the **whole** entity, not its centroid, so a
wall that straddles `z = 10` is not `above=XY.offset(10)`. For a planar face
`on_plane=` is exact; for a curved face and for edges it is measured at points
on the entity.

`kind="cylinder"`, `"cone"`, `"sphere"` and `"torus"` select faces by the
analytic surface they were **recognised** as (`cadpole_core::surface_class`):
every face is sampled and fitted at tessellation, and the class is
claimed only when every sample lies within a micron of it, so a loft band that
is *nearly* a cone is not one. `kind="plane"` includes a flat loft side that
truck holds as a B-spline; the truck-level strings (`"nurbs"`, `"bspline"`,
`"revolved"`) select by truck's own surface type. There is no face `radius=`
filter. Edge `"circle"` / `"arc"` and `min_radius=` are measured here
by fitting a circle to the edge and checking every point against it, because
`kernel::curve_kind` calls a circle a `bspline`.

**An empty result is a warning, not an error.** `len(sel) == 0` is a fair thing
to assert on, so the set may be empty; but it raises a warning naming the
filter that rejected everything and the closest miss, because a selector that
silently returns nothing is a trap. `allow_empty=True` says nothing is the
answer you wanted. An *operation* that consumes an empty selection — `fillet`,
`sketch(on=)`, `.one()` — is an error.

#### `Selection`

An ordered, deduplicated set of faces or edges of one body. It behaves like a
list — `len(sel)`, `sel[0]`, `for f in sel`, comprehensions, `sorted()`,
`"name" in sel` — and adds the two things a list cannot do.

| | Returns | |
| --- | --- | --- |
| `sel.count()` | int | same as `len(sel)`, reads better in an assertion |
| `sel.names()` | list[str] | the generated names, in the selection's order |
| `sel.kind` | str | `"face"` or `"edge"` |
| `sel.query` | str | the filters that produced it, for diagnostics |
| `a \| b`, `a + b`, `a.union(b)` | Selection | union |
| `a & b`, `a.intersection(b)` | Selection | in both |
| `a - b`, `a.difference(b)` | Selection | in `a` and not `b` |
| `sel.one()` | Face/Edge | exactly one, or an error naming what it found |
| `sel.nth(k)` | Face/Edge | the k-th in canonical order (negative counts back) |
| `sel.largest()` / `sel.smallest()` | Face/Edge | by area (faces) or length (edges) |
| `sel.nearest((x,y,z))` | Face/Edge | closest centroid |
| `sel.max(axis)` / `sel.min(axis)` | Face/Edge | furthest along `"x"`, `"y"`, `"z"`, `"-z"` or a vector |
| `sel.edges()` | Selection | the edges bounding the selected faces |
| `sel.faces()` | Selection | the faces the selected edges lie between |

Combining selections from two different bodies is an error, never a merge.

**The order is geometric and never the kernel's.** A selection comes back
sorted by centroid `(z, y, x)`, then lower bounding-box corner, then larger
first, then generated name: *lowest first, then front to back, then left to
right*. Coordinates are compared at micron resolution, so nominally-equal
positions cannot swap on float noise, and the final tie-break is a name that
depends only on the feature that made the geometry. A small parameter change
moves entities; it does not reorder them. `a | b` comes back in the order one
selector asking for both would have produced.

```python
b = box(20, 20, 10, name="b")
b.faces(normal=(0, 0, 1)).one()                  # the top face
b.faces(name="side[*]").count()                  # 4
caps  = b.faces(normal=(0, 0, 1)) | b.faces(normal=(0, 0, -1))
walls = b.faces() - caps                         # the four sides

# The Gridfinity floor-fillet chain: every edge on the pocket floor, straight
# runs and corner arcs alike, rounded in one call.
cut = extrude(sketch(XY.offset(1)).rounded_rect(30, 40, 3), 20, name="cut")
floor = cut.edges(on_plane=XY.offset(1))         # 8 edges
assert_true(floor.count() == 8)
rounded = fillet(floor, 2.55, name="rounded")    # exact: four cylinders and four torus quarters, one tangent chain (§4.4)
```

#### `Body.face(name)`

Accepts the full generated name (`"lid/end"`), the local name inside this
body's feature (`"end"`, `"top"`), an alias (`"end"` for a box's `"top"`), or
the `face:N` compatibility form. A name that stands for several faces — a
group such as `"side"` on a cylinder, or a name a boolean split — is an error
naming the pieces, never a silent pick. A name that matches nothing is an
error listing what the body does have.

`Face.name` is the generated name and `Face.aliases` the other names that
resolve to it.

#### `Body.edge(name)`

The same, for the edge named by the pair of faces it bounds
(`"lid/end|side[0.2]"`) or the `edge:N` compatibility form.

#### Topological name grammar

Names are paths from the *generator* feature, never from the last
transform/boolean, so a script can write `block.face("end")` before and after
booleans. **The grammar is defined in `docs/design.md` and nowhere else**;
what follows is how it looks in a script, not a second definition.

```python
b = box(20, 20, 10, name="blk")          # blk/top, blk/bottom, blk/front, ...
p = cylinder(3, 20, name="hole", at=(10, 10, -5))   # hole/top, hole/bottom, hole/side#0, hole/side#1
part = subtract(b, p, name="part")
part.face("blk/top")                      # the block's top survived the cut
part.face("hole/side#0")                  # the wall of the hole keeps the tool's name
part.edge("blk/top|hole/side#0")          # "where the hole meets the top"
extrude(sketch(XY).rect(20, 20), 5).face("side[0.2]")   # loop 0, segment 2
```

`face:N` / `edge:N` also address by index and resolve to the real name. See
`docs/design.md` for propagation and staleness rules.

### 4.9 Measurement

| Call | Returns |
| --- | --- |
| `Body.bbox()` | `((min), (max))` |
| `Body.volume()`, `Body.area()` | mm³, mm² |
| `distance(a, b)` | `Clearance`: the minimum distance between two bodies' surfaces, where it is, on which faces, and whether the bodies overlap |
| `clearance_map(a, b, *, below, spacing=1, limit=256)` | every region where the gap between two bodies is below `below` mm, closest first |
| `sightline(from, to, *, eye=, obstacles=)` | `Sightline`: is the target point visible from the eye point, and if not, which body and face stop the ray, where and how far out (§B8) |
| `sightlines(from, target, *, eye=, obstacles=, samples=256)` | `SightlineFan`: the fraction of a target body, face or point list an eye can see, and which bodies hide the rest (§B8) |
| `view_cone(from, *, eye=, az, fov=60, el=(0, 30), obstacles=, rays=32)` | `SightlineFan`: the fraction of a window of sky — a compass azimuth span and an elevation band — an eye sees past the bodies (§B8) |

`distance` takes two bodies; there is no distance between faces, edges or
points.

`volume()` and `area()` are measured on the body's **tessellation**, by the
divergence theorem and by triangle areas. They are therefore exact for a
polyhedron and carry the chord error for anything curved: a mesh is inscribed
in the true surface, so a cylinder's volume comes out slightly low and halving
`tolerance` roughly quarters the gap. They are for *comparing* two bodies built
the same way — that a cut removed nothing, that a pattern really made N copies
— not for a certified mass property. The Gridfinity
baseplate's drop test (`examples/gridfinity_baseplate.star`) is the worked
example: it fuses a seated bin foot into the cup-shaped cutter and asserts the
cutter's volume did not move, which is exactly "the foot fits inside the cup".

`bbox()` is read off the same tessellation, whose vertices are stored in
**single precision** (`f32`), so its corners carry f32's rounding: a
bound at 14325.6 mm comes back as `14325.599609375`, about a part in ten
million. Compare a bound with
`math.close(a, b, tol=1e-3)` or `assert_close`, never `==`, and write a
number you print with `format(x, ".1f")` (§4.11). A length the script
computed itself (a wall's `ft(30)`) is exact and is the better number to
dimension.

#### `distance(a, b)`

The minimum distance between two bodies' surfaces, and where it is: the fit
test a print-in-place part needs, which `intersect` cannot be (an empty
result is an error and a touching pair is a refusal) and the
union-volume idiom above only approximates. Returns a `Clearance`:

| Attribute | |
| --- | --- |
| `distance` | mm; `0` when the bodies touch or overlap |
| `overlap` | the bodies share material: one crosses the other, or sits inside it. A body in another's *cavity* is not an overlap |
| `touching` | zero distance without shared material |
| `at` | the closest pair of points, `(a_point, b_point)`; also `a_point` and `b_point` |
| `face_a`, `face_b` | the named faces those points lie on (§5): `sun/side[3]`, not a triangle index |
| `tolerance` | the tessellation tolerance the answer was measured at: the coarser of the two bodies' own (`body.tessellation(tolerance=)`, or the document's for a body without one; a rigid copy carries its source's) |
| `bodies` | the two names |

```python
c = distance(planet, ring)
assert_true(c.distance >= 0.15 - 2 * c.tolerance, msg="planet %s clears the ring by %s at %s" % (c.face_a, c.distance, c.at))
assert_true(not c.overlap)
```

`str(c)` reads `clearance(8.89 mm between a/side[1] and b/side[3], ±0.05 mm)`,
or, under the document's `units(...)` (§4.1), `clearance(0.35" between
…, ±0.002")` — a decimal in the document's small unit, never a fraction. The
attributes are millimetres either way.

**What it measures, and how far to trust it.** Both operands are the
bodies' tessellations, each at its own level (§6.1: the body's
`tessellation(tolerance=)`, else the document's) — the triangles `export`
writes and `print_check` judges — one axis-aligned box tree each, walked together
best-first on the distance between boxes, with an exact triangle–triangle
distance in the leaves. A chord lies inside its true
surface by up to the tessellation tolerance, so *each* surface may be off by
`.tolerance` and the true gap lies within twice it of `.distance`; planar
faces contribute nothing. A 5 mm cylinder at the default 0.05 mm is a
31-gon whose chords sit 0.026 mm inside the circle, and two such polygons out
of phase measure 0.108 mm where the model says 0.15 — inside the contract,
and the reason to give close-fitting bodies a finer
`body.tessellation(tolerance=)`. Two bodies at the same distance on several faces (two cubes
0.4 mm apart, whose side faces' edges lie on the facing walls) report the
pair whose faces *face each other*. When the surfaces meet, `overlap` comes
from a cheap test rather than a boolean: a pair of triangles passing through
each other's planes, or a few probe points just inside one body's surface
tested against the other's winding number.

#### `clearance_map(a, b, *, below, spacing=1.0, limit=256)`

Every region where the gap between `a` and `b` is below `below` mm, as a
list of `(distance, a_point, b_point, face_a, face_b)` tuples, closest first.
Space is divided into `spacing` mm cells and each cell keeps its closest
pair, then any entry within `spacing` of a closer one is dropped, so a pin in
a bore reports its ring once per millimetre of it rather than once per
chord. `limit` caps the list; when it does, a warning diagnostic says how
many regions there were (`clearance_map(pin, housing): 256 of 1834 regions
below 0.3 mm shown; pass limit= for more, or a larger spacing= to merge
neighbours`). Measured as `distance` is, with the same uncertainty.

```python
for d, pa, pb, fa, fb in clearance_map(sun, planet_1, below=0.3):
    print("%s and %s are %.2f mm apart at %s" % (fa, fb, d, pa))
```

### 4.10 The `math` namespace

Starlark ships arithmetic and nothing else: no square root, no trigonometry,
not even pi. `math` supplies them.

**Angles are degrees**, matching every other angle a Cadpole script writes —
`rotate(45)`, `revolve(angle=90)`, `--view el=-28`. That is the opposite of C
and of Python's `math`. `math.radians` and `math.degrees` convert for anyone
who needs the other one.

Every function is pure. None records a feature, and a domain error is a
diagnostic naming the call and the value, never a NaN that travels into the
kernel and fails somewhere unrelated.

| | |
| --- | --- |
| Constants | `math.pi`, `math.tau`, `math.e` |
| Roots and powers | `sqrt(x)`, `cbrt(x)`, `hypot(x, y)`, `pow(x, y)`, `exp(x)`, `log(x, *, base=e)` |
| Trigonometry, degrees | `sin(d)`, `cos(d)`, `tan(d)`, `asin(x)`, `acos(x)`, `atan(x)`, `atan2(y, x)` |
| Conversion | `radians(d)`, `degrees(r)` |
| Rounding | `floor(x)`, `ceil(x)`, `trunc(x)`, `round(x, digits=0)` |
| Comparison | `fabs(x)`, `sign(x)`, `clamp(x, lo, hi)`, `lerp(a, b, t)`, `close(a, b, *, tol=1e-9)` |

`math.close` is the one to reach for instead of `==` on two computed lengths,
which is a bug waiting for a parameter change. `math.tan` at a quarter turn,
`math.sqrt` of a negative and `math.asin` outside -1..1 are all errors rather
than infinities.

```starlark
def ngon(n, r):
    return [(r * math.cos(360.0 * i / n), r * math.sin(360.0 * i / n)) for i in range(n)]

plate = extrude(sketch(XY).polygon(ngon(12, 20)), 4, name="plate")
```

### 4.11 Formatting numbers: `format(value, spec="")`

Starlark's `%` operator takes `%s %d %f %e %g` with nothing between the `%`
and the letter, so `"%.1f" % area` is an error, and `str()` of a float
writes every digit it has (`26.24671916010498`) or goes scientific from a
million (`1.234568e+06`). `format(value, spec)` is Python's builtin of the
same name, its format-spec mini-language
`[[fill]align][sign][z][#][0][width][,|_][.precision][type]`:
`format(3.14159, ".2f")` is `"3.14"`, `format(1234567.8, ",.0f")` is
`"1,234,568"`, `format(0.256, ".1%")` is `"25.6%"`, `format(-1.5, "+.1f")`
is `"-1.5"`, `format(7, "03d")` is `"007"`, `format("A2.0", ">8")`
right-aligns. Types `f F e E g G %` for numbers, `d b o x X` for integers,
`s` for strings; no spec writes a float as Python's `str()` does
(`1234567.891`). Rounding is to the nearest of the exact binary value, as
Python's is: `format(2.675, ".2f")` is `"2.67"`. `math.round(x, 1)` rounds
the number; `format` writes it.

### 4.12 Materials and rendering

#### `Body.material(*, preset=None, color=None, roughness=None, metallic=None, texture=None, scale=None, depth=None, opacity=None, ior=None, emissive=None, transmission=None, attenuation=None, scatter=None, scatter_g=None, anisotropy=None, edge_break=None, label=None, shadow=None)`

Sets appearance; returns the same body (chainable). Not a feature. See §3
Material for the fields, the preset list and the texture names. `preset=` (a
`preset` member or its string, §4.16) is applied first and the other keywords
override it. An unknown preset or texture
name is an error naming the valid choices, and leaves the body's material
untouched. Naming a machining-mark texture without `scale=` sets the scale
to the 0.15 mm pitch of a real one; the albedo patterns keep whatever scale
the material had.

`shadow=False` makes the body **guide geometry**: the camera sees
it and light does not. In the path tracer no shadow ray stops at it or is
tinted by it, no bounce meets it and it is no light source even when
emissive; the rasterizer leaves it out of its sun-shadow BVH and its contact
shadow. Its own surface is still lit and shadowed by the real bodies, and
it is drawn exactly as before — the translucent stipple, the refraction,
the tint the eye sees through it are unchanged. The flag is part of the
material, so it survives `translate`, `rotate`, `mirror`, a `union` (the
first operand's material, as every field), a pattern copy and an instance,
and a `preset=` resets it as it resets every field. `shadow_study()`,
`sightline()`, `sightlines()` and `view_cone()` take explicit body lists,
and a body passed to them is a caster or an obstacle whatever the flag
says — the flag is about the picture, the numbers are about the bodies you
name. The web viewer draws no shadows, so it needs nothing from the flag.
`examples/hillside_lot.star`'s envelope uses it:

```python
envelope.material(color=(0.95, 0.75, 0.25), opacity=0.3, shadow=False)   # seen, never shadowing the house
```

#### `render_defaults(*, sun=None, north=None, camera=None)`

The document's own render settings, which every render of it takes unless
the request says otherwise: `sun=sun(...)` lights the scene with that sun
and its shadows (`cadpole render --sun` wins over it; a sun below the horizon
lights nothing); `north=` says where north is in model space, in degrees
clockwise from `+y` seen from above (0: `+y` is north, the default; 90:
`+x` is north), and turns the sun to match (`--north` wins); `camera=
view(eye=, target=, fov=)` is a free camera — a neighbour's window, a point
on the sidewalk — that `cadpole render`, `cadpole sweep` and `cadpole animate` use
when no `--view` or `--eye` is given. A declaration, not a feature: it
builds nothing, and a document makes it once. Because the sun is computed
from the document's parameters, `cadpole sweep --param hour=9,12,15` is a
shadow study and an `animation` over an `hour` parameter is a day's sun
sweep (§4.17). The model carries it as `Model.render_defaults`.

```python
hour = param("hour", 12.0, unit="h", min=5, max=21)
render_defaults(sun=sun(38.0, -121.0, "2026-12-21", hour, tz=-8), north=0,
                camera=view(eye=window, target=(45 * ft, 60 * ft, 0), fov=70))
```

#### Cameras, renders and parameter studies

There is no `camera()`, `render()` or `sweep()` global. The free camera is `view(eye=, target=, fov=)`
(§4.17), taken by `render_defaults(camera=)` and by a keyframe, or `cadpole
render --eye x,y,z --target x,y,z` on the command line. A parameter study
is `cadpole sweep --param w=30,40,50 --param h=10,20`, which renders the
cartesian product into a labelled contact sheet.

### 4.13 Layers

A display layer is a named group the web UI's Layers panel switches on and
off and ghosts **without re-evaluating the script**, and that `cadpole render
--layer name=off|on|0.4` controls the same way for an image. It is the tool
for "show me the site without the envelope": a bool `param()` and an `if`
re-run the script and take the body out of the model; a layer leaves the
model alone and changes the picture. Nothing about a layer reaches
geometry or the memo; a whole-document export and `cadpole check` take what a
viewer shows (below), and a script can list a layer's bodies
(`bodies(layer=)`, below).

#### `layer(name, *, doc=None, visible=True, opacity=1.0, color=None)`

| Arg | Type | Notes |
| --- | --- | --- |
| `name` | str | unique per document; the panel's label and `--layer`'s name |
| `doc` | str | the panel's tooltip |
| `visible` | bool | the layer's own default; `False` declares it hidden until a switch, `--layer name=on` or `view(layers=)` shows it |
| `opacity` | number | 0..1, the layer's own default; below 1 the layer's bodies are ghosted, translucent the way `material(opacity=)` is |
| `color` | `"#rrggbb"` or `(r, g, b)` | a tint for the panel's swatch; the bodies keep their own materials |

Returns a `Layer` (§3). Declaration order is the panel's order. Declaring a
name twice with the same settings is the same layer, so a library and the
document that loads it may both declare `"fixture"`; with different
settings it is an error naming the layer and the first declaration's line.

**Joining a layer.** Three ways, all a tag on the body:

- `layer=` on every body-producing builtin that takes `name=` — `box`,
  `cylinder`, `cone`, `sphere`, `torus`, `hull`, `extrude`, `revolve`,
  `loft`, `sweep_along`, `mesh`, `union`, `subtract`, `intersect`, `glue`,
  `split`, `translate`, `rotate`, `copy`, `mirror`, `scale`,
  `orient_for_print`, `fillet`, `chamfer`, `imprint`, `emboss`,
  `pattern_linear`, `pattern_circular`, `pattern_along`,
  `pattern_on_surface`, `import_step`, `import_mesh`, `Body.translate`,
  `Body.rotate`, and `instance(...)` for every posed body of the instance —
  taking a `Layer` or a declared layer's name: `box(10, 10, 10,
  layer=envelope)`, `box(10, 10, 10, layer="envelope")`.
- `paint(body, layer=envelope)`: on its own, with no `color=`, a tag and
  nothing else (no feature); with `color=` the paint is recorded as well.
- `envelope.add(a, b)`: tags and returns the bodies (`add(a)` returns `a`,
  `add(a, b)` a list); takes Instances too.

A body is on one layer at a time; tagging it again moves it. A body on no
layer is on the **implicit layer**, which the panel lists as "Other" only
when a named layer exists, and which `other` names on the command line and
in `view(layers=)` unless the script declares a layer called `other`.

**Propagation follows the material's rule**: a transform, a copy, a pattern's copies, a mirror, a scale, a split's
pieces, a fillet, a chamfer and an emboss carry their source's layer; a
boolean's result takes its **first operand's** (`subtract(part, tool)` stays
on `part`'s layer whatever `tool` is on); an instance's posed bodies take
the part body's layer unless `instance(layer=)` says otherwise, and keep it
through `place` and `pose`.

**Refused by name:** putting a body on a layer nothing declared (the
declared names are listed), a redeclaration that disagrees, an opacity
outside 0..1, a `layer=` that is neither a `Layer` nor a string, an empty
name.

```python
part_layer = layer("part", doc="The machined part")
envelope = layer("envelope", doc="The space the part may claim", opacity=0.3, color="#ff8800")
fixture = layer("fixture", doc="The plate and pins it is held by", visible=False)

blank = box(60, 40, 24, center=True, name="blank", layer=part_layer)
part = subtract(blank, cylinder(6, 26, at=(0, 0, -1)), name="part")   # on "part": the first operand's
space = box(72, 52, 30, center=True, name="space", layer=envelope)
fixture.add(box(100, 80, 6, at=(0, 0, -6), center=True, name="plate"))
```

Offline: `cadpole render examples/layers_demo.star --layer fixture=on --layer
envelope=0.1` (docs/views.md); in an animation, a keyframe's
`view(layers=)` (§4.17).

#### `bodies(*, layer=None, visible=None, name=None)`

The document's bodies so far, in the order they were made, as a list of
`Body` values: how a print script that `load()`s a model picks what to
print without a second pass through `cadpole eval --json`.

| Arg | Type | Notes |
| --- | --- | --- |
| `layer` | `Layer`, str, or a list of them | keep the bodies on these layers; `"other"` is the bodies on no layer (as `cadpole render --layer other=off` spells it). An unknown name is an error listing the declared layers |
| `visible` | bool | `True` keeps the bodies a viewer shows and a whole-document export writes -- not consumed by a boolean or a split, not on a hidden layer; `False` the rest; left out, both |
| `name` | str | a glob on the body's name: `*` any run, `?` one character |

Filters combine: every one given must hold. The list is a snapshot -- a
body made after the call is not in it -- so call it straight after `load()`
for the loaded model alone; the bodies a print script derives later carry
their source's layer (a copy, a cut piece, a boolean's first operand).
Nothing is built or consumed. With it, `Body.layer()`, `Body.visible()` and
`Body.color()` (§3) read one body's state when called.

```python
load("house.star", "HOUSE")                          # the model, drawn in feet
shell = bodies(layer=["walls", "floors"], visible=True)
glass = bodies(layer="walls", name="*_glass*")       # left out of the print
keep = [b for b in shell if b not in glass]
```

**Choosing what `cadpole check` and `cadpole export` take.** Both take
what a viewer shows by default: every body not consumed and not on a
hidden layer.
`--include-hidden` adds the hidden layers. `--layer NAME` (repeatable)
takes the bodies on that layer -- a hidden one too, when it is the one
named -- and `--body ID` (repeatable) a body by its id (`feature#n`) or its
name; the two add up (`--layer walls --body slab#4` is the walls and the
slab). A name that picks nothing is an error rather than an empty file or
an empty report. The `PrintCheck` and `Export` operations take the same as
`layer_names`, `body_ids` and `include_hidden_layers`; `cadpole op PrintCheck
--layer walls` works too. `--layer` here is a name; `NAME=on|off` is
`cadpole render --layer`, which switches a layer in a picture, and is refused
here with that said.

```
cadpole check print_kit.star --layer kit                # only the print pieces, not the loaded model
cadpole check print_kit.star --body L0 --body L1        # two pieces by name
cadpole export house.star --layer walls -o walls.3mf     # one layer's bodies
```

**Layers in exports.** `cadpole export` (the `Export` operation)
leaves out the bodies on a layer the script declares hidden, in every
whole-document format -- STL, OBJ, 3MF, glTF, STEP -- unless
`--include-hidden` (`include_hidden_layers`) asks for them. Bodies named
by id are written whatever their layer, and a script's own `export(...)`
calls write what they name. Where the format has a place, the layer goes
with the body: OBJ writes `o <body>` then `g <layer>` (a `# hidden layer`
line marks a body written only because it was asked for); 3MF names each
object's layer in `cadpole:layer` object metadata; glTF puts every body's
node under one node per layer, as below.

#### glTF out: `cadpole export doc.star -o doc.glb` (or `.gltf`)

A scene for web viewers, game engines and DCC tools, written by Cadpole's
own writer. One root node named for the document; under it one node per
layer that holds a written body, in declaration order; under each, one
node per body, named for the body, its mesh named the same. A body on no
layer hangs off the root. Each node's `extras.cadpole` says what it is: a
layer node's `layer`, `visible`, `opacity`, `color`; a body node's
`body` (its id), `layer` and `hidden`. One material per distinct Cadpole
material, named as the script named it (one a script coloured without
naming is named by its colour, `#rrggbb`): base colour with opacity as
alpha, metallic, roughness, emission. A scan's colour per vertex is
`COLOR_0`.

glTF is metres with +Y up, so Cadpole's `(x, y, z)` millimetres are written
as `(x, z, −y)` / 1000 -- exactly what `import_mesh` (§4.15) reads
back to where it was. A script drawn in feet needs `--model-unit ft`
(mm, cm, m, in or ft), as `cadpole bake` does; `--units` sets the display
units recorded in `asset.extras.cadpole` (default: the script's `units(...)`,
else ft-in for feet or inches, else mm). The output is deterministic.
`--model-unit` and `--units` are glTF only and refused for the other
formats.

```
cadpole export examples/cabin_site.star --model-unit ft -o cabin.glb                  # the hidden shed left out
cadpole export examples/cabin_site.star --model-unit ft --include-hidden -o cabin.gltf # the shed's bodies marked hidden
cadpole export examples/cabin_site.star -o cabin.obj                                  # o/g by body and layer
```

Not written: procedural textures, transmission, paint; a script's own
`export(b, "x.glb")` is not accepted.

### 4.14 Libraries and `load()`

`load("lib.star", "helper")` loads a sibling script of the document (the
packed document's script list, or the `.star` files next to `main.star`). Loaded
files evaluate once, against the same feature tree, so a library function that
calls `extrude` adds features exactly like inline code. `//` and `./`
prefixes are accepted and stripped; cycles are errors.
`load("nightstand.star", "NIGHTSTAND")` is a *part file*: loading it builds
the nightstand, on purpose (`examples/nightstand_scene.star`).

**A relative path is taken from the loading file's directory**:
`load("../model/site.star", "house")` from `sheets/a.star` finds the
model beside the sheets, and a `load("dims.star")` inside `site.star` is
`../model/dims.star`, not a file of the same name next to the entry.
`cadpole` on a `.star` file
bundles every file its `load()`s reach, transitively, under those keys, so
a sheet script in its own directory loads its model and its library with
no symlinks and no absolute paths.

**A loaded file's exports are its own document's**: its
`export()`, `export_svg/pdf/dxf()` and `export(set, ...)` calls run when it
is the entry and are dropped, with one info diagnostic, when it is loaded,
so a sheet script that loads the model does not re-write the model's STEP
files.

Conventions for libraries:

- Functions take a `name` keyword and forward it, so callers control feature
  ids: `def peg(r, h, name=None): return cylinder(r, h, name=name)`.
- Return the bodies they make; never rely on globals.
- Prefix any feature they create without a caller-supplied name with the
  library's name to avoid collisions (`name="gears_" + ...`).

#### The standard library

`load("@cadpole/gears.star", "spur")` loads a module that is **built into the
engine**: the files under `stdlib/` are compiled into `cadpole-script` at build
time, so the CLI, `cadpole serve`, the C FFI and the wasm engine carry the same
library at the same version (`cadpole --version` is the library version), the
document never ships a copy, and resolution needs no network and no path on
the machine. The model in five lines:

1. `@cadpole/<name>.star` is the only prefix that resolves; a document's own
   files are bare names, and a document file may not be named under `@`.
   A missing module lists the modules the engine has.
2. **Loading is pure.** A library records no feature, declares no parameter
   and asks for no export when it is loaded; the loader checks the store's
   counts before and after and refuses otherwise, and `param()` refuses to
   run from a library file. A library takes values; the document declares
   the parameters.
3. Every function takes `name=` and forwards it; its docstring's `Names:`
   line is the list of feature ids and face names it promises.
4. An error raised inside a library reports the library file and line *and*
   the call in the document, and the diagnostic's span is the document's
   line: `gears.spur: the root fillet does not fit … (in
   @cadpole/gears.star:345, called as spur from main.star:3:7)`.
5. A `test()` declared in a library would run in every consumer's `cadpole
   test`, so the libraries keep their tests next to them
   (`stdlib/<name>_test.star`, run by `cadpole_script_test`).

There is no loading by URL, no user-wide library directory and no
auto-loading. Modules:

| Module | Contents | Probe |
| --- | --- | --- |
| `@cadpole/gears.star` | involute gears, §P5 | `pitch_diameter` |
| `@cadpole/mechanical.star` | springs, threads, bolts and nuts on `sweep_along` and `helix`, §P6 | `thread_depth` |
| `@cadpole/site.star` | site modelling on `terrain()`: a local frame from latitude and longitude, parcels with named sides, setback and rear-yard lines, a profile placed on a lot and the grade read back along it, the envelope above grade, cut and fill, neighbour masses, §B6 | `polygon_area` |
| `@cadpole/building.star` | architecture massing: feet and inches, levels and floors, hip/gable/shed roofs by straight skeleton, decks, stairs, shafts, floor areas with the 5 ft rule, lot coverage, height envelopes over grade with averaging, a floor-area cap; §B4 | `ft` |
| `@cadpole/print.star` | printing a model in pieces: registration pegs and sockets sized for FDM, the places they fit, story bands cut, joined with a weld, pegged and dropped onto the bed; §6.2 | `is_box` |
| `@cadpole/plan.star` | the floor-plan value every `@cadpole/plan*` module shares: plain-data records (floors, rooms, doors, windows, fixtures, stacks, courts) with constructors and `validate()`; areas, shared and exterior walls, the door graph and its paths, a room in its own frame; `ftin` and `sqft`; §B10 | `ftin` |
| `@cadpole/plan_draw.star` | a plan value drawn on sheets: every floor in layers -- rooms filled by kind, walls derived from the rooms, doors with their swings, windows, fixtures, stacks, a label per room placed clear of the rest -- a red pen of numbered check rows, plan sets and a comparison sheet with scorecards; §B10 | `kind_fill` |
| `@cadpole/plan_audit.star` | the red pen for a plan: code (R304, R305, R310.1, R311.6, R311.7, CPC 402.5, R302.1), the door graph, plumbing stacks, filler rooms, daylight and furniture, as check rows and marks; §B10 | `ftin` |
| `@cadpole/plan_light.star` | daylight for a plan: lit depth within 2H of facade glass, the sky a court, well or slot window sees by `view_cone` against a street window's, a sky table over court width, wall height and floor; the light rows the audit reports; §B10 | `lit_depth` |
| `@cadpole/plan_furniture.star` | the room stage's catalog as data: furniture and fixtures with sizes, use spaces, heights and sources (NKBA, CPC 402.5, 407 and 408, the IRC's R307), and the sets a room holds; §B10 | `catalog_probe` |
| `@cadpole/plan_furnish.star` | the room stage: a room's furniture or fixtures packed by a MILP clear of door swings, windows and stacks, re-walked at its final size; least depths, fit tables and `fit_row`; §B10 | `furnish_probe` |
| `@cadpole/plan_fit_tables.star` | the room stage's fit tables as data: the least depth that holds each set by clear width and door class, 170 entries and a 6-in grid; generated by `plan_fit_tables_gen`; §B10 | `fit_tables_probe` |
| `@cadpole/plan_fit_tables_gen.star` | the generator of `@cadpole/plan_fit_tables.star`: the fit-table jobs, solved by the room stage and written out as Starlark; the stretch test; §B10 | `fit_jobs` |
| `@cadpole/plan_kitchen.star` | one kitchen run against NKBA's landing areas: the shortest compliant run, the least shortfall at a given length, a drawn run checked; §B10 | `kitchen_probe` |
| `@cadpole/plan_roof.star` | PV rows on a roof plane: modules round skylights and hatches, inside R324.6 / CFC 1205's pathways and ridge setbacks, with the low-slope and sprinklered rules; §B10 | `roof_probe` |
| `@cadpole/plan_solve.star` | floor plans as MILPs on `opt`: free rectangles in zones, exact areas, walls between rooms, doors as a tree, escape walls, L-pieces, must-holds; stacks, bearing lines, beams, transfers, not-over, courts and moving faces across floors; `solve`, options by arrangement cuts, floor by floor; §B10 | `ftin` |
| `@cadpole/plan_ask.star` | ask the solver about your own plan: pin its arrangement and size it, check it, cost a rule by paired solves, explain a clash by rule family, edit it moving the fewest feet of wall, local LP duals on the pinned plan; changes as data; §B10 | `describe` |
| `@cadpole/plan_frame.star` | a fixed plan's cheapest frame: `reframe()` picks bearing lines, joist sizes and spacing, beams, transfers and headers as a MILP over joist bays; members by name, span and size; framing plans; a planning aid, not engineering; §B10 | `frame_menu` |
| `@cadpole/plan_spans.star` | span tables as data: CRC R502.3.1(1) and (2) floor joists, R802.4.1(2) rafters, R602.7(1) and (2) headers and girders; I-joists, LVL and steel by rule of thumb; `ftin`; §B10 | `joist_span` |
| `@cadpole/plan_parti.star` | parti-first floor plans: a template's column grid, stair, court and rules fitted on every floor at once by one MILP; `fit`, `compare`, `explain` (a deletion filter by rule family), `author`, `precheck`; §B10 | `grid` |
| `@cadpole/plan_templates.star` | the parti library for 25-ft row-house lots: side hall, cross stair, middle court and open core as data; mirror, column splits, a neighbour's light well, an optional court, four floors; §B10 | `side_hall` |
| `@cadpole/plan_rules.star` | the vetted rule menu for generated plans: typology rules hard, size rules weighted, the standing set; the same rules as checks on any plan; §B10 | `rule` |

The probe is the pure function `cadpole_script_test`'s
`the_real_library_declares_no_parameter_and_none_may` loads from each module
to check that loading it records nothing; the probes and the one-line
summaries (which a `load()` of a missing module lists) live in
`src/rust/cadpole-script/src/stdlib.rs`'s `MODULES`.

Each module is documented with the work it belongs to: gears in §P5 and
`mechanical` in §P6 (`parts.md`); `building` in §B4, `site` in §B6 and the
`plan` family in §B10 (`buildings.md`); `print` in §6.2.

### 4.15 Assets, imports and text

Files the script did not write — an SVG logo, a floor-plan DXF, a font — are
**assets**: bytes the document carries, named by the user and pinned by
SHA-256. Scripts never see a path, so the same script gives the same model in
the CLI, in the browser and on a render host.

#### `asset(name, *, sha256=None)`

```python
logo = asset("badge.svg")          # a name in the document's asset table
plan = asset("house.dxf", sha256="9ee7dae2…")   # assert the version you wrote against
```

Attributes: `name`, `sha256`, `mime`, `kind` (`"svg"`, `"dxf"`, `"stl"`,
`"obj"`, `"3mf"`, `"gltf"`, `"step"`, `"image"`, `"font"`, sniffed from the
bytes, not from the file name) and `size`. A name that is not in the table is an error
listing the names that are; a name whose bytes did not travel with the
document is a different error that says so. A `sha256=` mismatch is a
**warning**, never an error: the file has changed, and you should look, but
the model still evaluates.

`cadpole eval main.star` fills the table from `./assets/` next to the script and
packs it into the document, so a loose script under git works with no
ceremony. A packed document embeds the bytes; transport sends them once by hash.

**How large.** Wherever bytes travel — an upload to `cadpole serve`, a packed
document, the browser — an asset is at most 32 MB and a document's assets
128 MB. A native run that reads `./assets` from disk (`cadpole eval`,
`render`, `serve`, `export`, … on a `.star`) holds a **mesh** — STL, OBJ,
3MF, glTF, and a `.bin` a `.gltf` names — to 512 MB and anything else to
32 MB, and a document to 2 GB (a phone's room scan is routinely past
32 MB). `cadpole --max-asset-mb N` (or
`CADPOLE_MAX_ASSET_MB=N`) sets one cap of N MB for every kind and four times
it for the document. A file over its cap is left out with a warning that
says which cap and how to raise it, and `asset("name")` then says the
document has no such asset. In the web UI a document with assets is
evaluated by `cadpole serve`, which holds them; the browser never does.

#### `import_svg(asset, *, ids=None, layer=None, plane=XY, at=(0,0), align="min", scale=1, units="auto", dpi=96, flip_y=True, fill_rule="auto", merge=True, tolerance=0.01, name=None)` → Sketch

```python
art = import_svg(asset("badge.svg"), ids=["badge"], align="center", name="art")
badge = extrude(art, 2.4, name="badge")
```

Filled shapes become loops and nested subpaths become holes; unfilled ones
(`fill="none"`) become construction paths (`sketch.paths`) that bound nothing.
The full path grammar is read — `M L H V C S Q T A Z` and every relative form
— along with `rect` (with `rx`/`ry`), `circle`, `ellipse`, `line`, `polyline`,
`polygon`, `g`, `use`, `symbol` and nested `svg`, with transforms composed.

**Arcs stay arcs.** An `A` command with equal radii, a `circle`, a rounded
`rect` corner and a circular `ellipse` all come in as circular arcs, so a hole
extrudes to a real cylinder. Elliptical arcs, and any arc under a shear or a
non-uniform scale, become cubic Béziers within `tolerance` and say so.

Units come from the file's `width`/`height` when they carry one (`120mm` plus
a viewBox is exact); otherwise user units are CSS pixels at `dpi` and an info
diagnostic says so — pass `dpi=72` for Illustrator or `units="mm"` to settle
it. `flip_y` (default) mirrors SVG's downward y into sketch y-up. `align`
places the result: `"min"` puts its bounding-box corner at `at`, `"center"`
its centre, `"none"` keeps the file's own coordinates and offsets by `at`.

Fill rules only matter where loops actually cross. When they do, the element's
`fill-rule` (or your `fill_rule=`) decides and the loops are rebuilt as
polygons, with an info diagnostic — the rebuild is the point where a curve
stops being a curve, so it never happens when it is not needed. `merge=True`
unions filled shapes from different elements that overlap, so `extrude` sees
one region instead of two bodies fighting over the same area.

Ignored, with an info diagnostic: `text`, `image`, filters, masks, clip paths,
patterns, gradients and `<style>` rules. The file is parsed, never rendered:
DTDs are refused, and `use` resolves only inside the document.

#### `import_dxf(source, *, layers=None, units="auto", name=None)` → Drawing2D

```python
site = import_dxf("site_survey.dxf", name="site")        # the asset's name, or asset("...")
ft = site.scale                                            # mm per drawing unit: 304.8, $INSUNITS said feet
lot = site.sketch(layer="PL", closed=True)                 # the property lines: one loop, site/PL/loop[0]
frame = extrude(subtract2d(lot, lot.offset(-4 * ft)), ft, name="frame")
shots = site.spot_elevations(layer="SPOT")                 # [(x, y, z), ...] in mm, from the TEXT labels
lines = site.contours(layer="CONTOUR*")                    # [(z, [(x, y), ...]), ...]
walls = extrude(import_dxf("house.dxf", layers=["A-WALL*"]), 2700, name="walls")   # a Drawing2D stands in for a Sketch
```

An ASCII DXF (R12 through 2018) as a **`Drawing2D`**: the file is parsed
once — layers, entities in millimetres, labels and points — and the script
then *questions* the drawing, layer by layer, through views that are pure
functions of it: `.sketch()`, `.curves()`, `.points()`, `.texts()`,
`.spot_elevations()`, `.contours()`, `.bbox()`, `.layers()`. `source` is an
`asset(...)` or the asset's name — the file under `./assets` next to the
script — and its `kind` must sniff as `"dxf"` (a STEP or a mesh handed to
`import_dxf`, or a DXF handed to `import_step`, is refused naming the call
to use). The import is one feature and one memo op keyed by the asset's
content hash, `layers`, `units` and the importer's version: a re-evaluation parses nothing and repeats the same
diagnostics, and every view on a memo hit reads the same parsed drawing.
The feature records `asset`, `sha256`, `layers`, `units`,
`importer_version` and what it read (`entities`, `unit`, `unit_source`,
`layers_found`, `layer_names`, `texts`, `points`, `blocks`, `inserts`,
`version`, `skipped`); each `.sketch()` call adds `sketch[k].layers`,
`.loops`, `.paths`.

**Attributes:** `id`, `units` (`"ft"`), `unit_source` (`"$INSUNITS"`,
`"units="` or `"assumed"`), `scale` (millimetres per drawing unit),
`entities`, `layer_names`, `version` (`"AC1024 (2010)"`) and `extents`
(`$EXTMIN`/`$EXTMAX` as the header declared them, in mm, or `None`).

**Units.** `$INSUNITS` decides: feet, inches, yards, the US survey foot,
millimetres, centimetres and metres are converted, and an info diagnostic
says so; a file that declares none (very common) is read as millimetres
with a **warning** that says how to settle it; `units=dxf_units.ft` (or
`inch`, `m`, `yd`, `survey_ft`, …) settles it either way and, when it
overrides a header that said otherwise, says so. A `$INSUNITS` Cadpole does
not convert (light-years, angstroms, …) is an error naming the unit and
the `units=` to pass. Every coordinate, elevation and text height a view
returns is in millimetres; `scale` is the number the script multiplies its
own feet by, so a survey in feet and a script in feet agree without a
constant. Unlike `import_svg` there is no `scale=` or `flip_y=`: a
drawing's own coordinates and orientation mean something.

**Picking layers.** Two places, for two reasons. `layers=` on the import
is what the drawing *keeps* — what is parsed, memoised and listed — and
`layer=` on every view is a subset of that. Both take a name or a list of
names, case-insensitive, with `*` and `?`, matched against the file's own
spelling and against the name Cadpole shows (the file's spelling with the
name grammar's reserved characters and whitespace replaced by `_`, so
`A-WALL` and `CONTOUR-150` are themselves and `Site Plan|1` is
`Site_Plan_1`). A name that matches nothing is an error listing what there
is. `.layers()` is how to find out: every layer the import kept, in the
file's order, as `(name, entity_count, (r, g, b))` — the count is every
entity on the layer in the file, the colour the layer's ACI index as RGB —
so a script can look before it selects. The file's `0` layer (every DXF
has one) is listed like the rest; off and frozen layers are read like any
other. Surveys and plans have conventions worth knowing: a plat's property
lines are on a `PL`, `PROP` or `BNDY` layer, contours on `CONTOUR`,
`C-TOPO` or `TOPO-<elevation>`, spot shots on `SPOT`, `ELEV` or `TEXT`,
control on `CONTROL` or `BM`; an architect's plan uses the NCS names
(`A-WALL`, `A-DOOR`, `A-GLAZ`, `A-ANNO-*`), which is what `export_dxf`
writes. Entities on layers `layers=` did not keep are counted in an info
diagnostic, never silently dropped. These are the *file's* layers, not
the display layers `layer(...)` declares (§4.13): `import_dxf`
and its views make no body, so they take no display layer; the builtin
that builds a body from the drawing does — `extrude(site.sketch(layer=
"PL"), 300, layer=lots)`, with `lots = layer("lots")` declared — and no
DXF layer becomes a display layer by itself.

**How entities become sketch curves — `.sketch(*, layer=None, closed=None, plane=XY, at=(0,0), align="none", tolerance=0.01)` → Sketch.**
Every closed entity — a closed `LWPOLYLINE`/`POLYLINE`, a `CIRCLE`, a full
`ELLIPSE`, a closed `SPLINE`, a `SOLID`, a `HATCH` boundary — is a loop on
its own. Every open one — the bag of `LINE`s and arcs a real plan is — is
joined end to end within `tolerance` mm: a chain that closes is a loop, one that does not is a
**construction path** (`sketch.paths`), which bounds nothing and is never
extruded; an info diagnostic counts them and says to raise `tolerance=` or
fix the drawing. `closed=True` keeps only the loops, `closed=False` only
the chains. Loops are named `<layer>/loop[k]` and paths `<layer>/path[k]`
under the import's feature (`site/PL/loop[0]`), so a face extruded from
one traces back to its layer. `plane`, `at` and `align` place the result
as `import_svg` does, except that the default `align` is `"none"`: the
drawing's own coordinates are kept and only offset by `at` (`"min"` and
`"center"` are there for a symbol). A `Drawing2D` is accepted wherever a
Sketch is — `extrude(import_dxf("plan.dxf", layers=["A-WALL*"]), 2700)` —
as its `.sketch()` with the defaults: every kept layer, loops and paths,
on XY, in the drawing's coordinates.

**Exact where the file is exact.** A polyline bulge is a circular arc
(`bulge = tan(θ/4)`), so a filleted corner extrudes to a real cylinder; an
`ARC` is an arc and a `CIRCLE` two of them; a `SPLINE` and an `ELLIPSE`
become cubic Béziers within `tolerance` (a degree ≤ 3 non-rational spline
by knot insertion, exact; anything else sampled). An `INSERT` of a block
is expanded where it stands with its insertion point, scale, rotation,
mirror (a negative scale) and column/row arrays, blocks nested eight deep
and never through a cycle; a rotated, mirrored or uniformly scaled arc is
still an arc, and a non-uniform scale turns a circle into the exact
ellipse it really is (`c + u cos t + v sin t`, closed under every affine
map) — only a bulge inside a non-uniformly scaled block is flattened
within `tolerance`, and counted. Entities on layer `0` inside a block take
the `INSERT`'s layer; `BYBLOCK` colours take its colour. What Cadpole's own
`export_dxf` writes reads back to the same loops within 1e-9 mm, six exact
arcs on a rounded plate, the figured text and the arrowheads of a
dimensioned sheet included (`cadpole_import_test`).

**The other views.** `.curves(*, layer=None, tolerance=0.01)` → every
geometry entity as a planar `Curve` in world space, in file order, named
`<layer>/<kind>[i]` (`PL/lwpolyline[0]`, `CONTOUR-152/line[3]`) under the
feature; an entity whose points all sit at one elevation is drawn on XY
lifted to that z, so a 3D contour polyline is a curve at its height, with
`.length`, `.closed`, `.start`, `.end` as any Curve has, ready for
`sweep_along` or `pattern_along`. `.points(*, layer=None)` → the `POINT`
entities and the vertices of every line and polyline that carries an
elevation (any vertex off z = 0), as `(x, y, z)`: a survey's shots taken
as points and its 3D breaklines, which feed `terrain(points)` directly.
`.texts(*, layer=None)` → every label — `TEXT`, `MTEXT` with its
formatting codes stripped, and the `ATTRIB` values a block's tags carry —
as `(x, y, text, height, rotation)`: the placement point in mm (the
alignment point of a justified `TEXT`), the plain text, the text height in
mm, the rotation in degrees. `.bbox()` → `((x0, y0, z0), (x1, y1, z1))` of
everything imported, curves sampled, or `None`; `.extents` is what the
header claimed instead.

**Spot elevations — `.spot_elevations(*, layer=None, pattern=None)` → `[(x, y, z)]`.**
A survey writes an elevation as a label beside the shot, and this view
reads them: every label that reads as one, at the label's placement point,
the number scaled to millimetres as the coordinates were. **The
convention** (no `pattern`): the label's words hold exactly one number,
with a fractional part (`158.14`, never a lot number `12`), and at most
two short alphabetic codes beside it — the `BK`, `TC`, `FL`, `EG`, `TW`,
`INV` a survey writes next to a shot — so `158.14 BK`, `TC 157.99`,
`158.14` and `BM 147.52` all read as elevations and `LOT 12`, `N 89°30'00"
E 110.00'` and `SEE NOTE 4.1 BELOW` do not; a trailing foot mark is
allowed. Keep it to the survey's spot-elevation layer with `layer=`, and
look at `.texts()` when a count is not what the sheet says. `pattern=` is
a template for any other spelling: `#` for a number (the first `#` is the
elevation), `*` for any run of characters, `?` for one, whitespace for a
run of whitespace, anything else itself, case-insensitively — `"EL #"`,
`"# BK"`, `"*=#"`; a pattern without a `#` is an error.

**Contours — `.contours(*, layer=None, tolerance=0.01)` → `[(z, [(x, y), ...])]`.**
Every polyline, line or spline on the layers with its elevation, in file
order: from the entity's own z when every vertex shares one off zero (a
3D polyline, an `LWPOLYLINE` with an elevation), else from the **last
number in its layer's name** (`CONTOUR-150`, `C-TOPO-152.5`,
`TOPO_1450`), scaled like the coordinates; an entity with neither is
skipped. Each comes back flattened within `tolerance`, a closed contour
with its first point repeated at the end. `terrain_from_contours`
wants `(x, y, z)` per vertex, which is one comprehension away:
`[[(x, y, z) for (x, y) in pts] for (z, pts) in site.contours(layer="CONTOUR*")]`.

**What is read, and how.** Everything a 2D drawing carries; each row is a
DXF entity type.

| DXF entity | Becomes | Notes |
| --- | --- | --- |
| `LINE` | a line segment | z kept per end (`.points()`) |
| `LWPOLYLINE` | lines and **bulge arcs**, a loop when closed | the elevation (code 38) is every vertex's z; widths ignored |
| `POLYLINE` / `VERTEX` / `SEQEND` | as `LWPOLYLINE` | 2D and 3D polylines; a polyface or polygon mesh is skipped as `POLYLINE_MESH` |
| `ARC` | one circular arc | counter-clockwise from start to end angle, as DXF defines it |
| `CIRCLE` | two arcs, a loop | stays a circle under rotation and uniform scale; an ellipse under a non-uniform one |
| `ELLIPSE` | cubic Béziers within `tolerance` | exact parametrised form kept until flattening, so a transformed ellipse is still the true ellipse |
| `SPLINE` | cubic Béziers within `tolerance` | control points + knots (+ weights), or fit points; degree ≤ 3 non-rational is exact by knot insertion; up to 10 000 points |
| `POINT` | `(x, y, z)` in `.points()` | |
| `TEXT` | a label in `.texts()` / `.spot_elevations()` | the alignment point of a justified text is its position; rotation kept |
| `MTEXT` | a label, formatting codes stripped | `\P` is a newline; the `\A;`, `\f;`, `\H;`, `\C;`, `\W;`, `\Q;`, `\T;` codes, the `\L`/`\O`/`\K` toggles and `{}` groups are removed, a `\S` stack is flattened, `%%d`/`%%p`/`%%c` become °, ±, Ø; chunks (code 3) joined |
| `ATTRIB` | a label: the value a block's tag carries | read with the `INSERT`; `ATTDEF` templates are counted, not read |
| `SOLID` / `TRACE` | a closed loop (three or four corners) | how a DXF draws a filled arrowhead |
| `HATCH` | its boundary loops | the pattern is ignored; polyline boundaries (with bulges) and line, arc, ellipse and spline edges |
| `INSERT` | the block's entities, transformed | scale, rotation, mirror, column/row arrays; 8 deep; a cycle or an undefined block is a warning |
| `BLOCK` / `ENDBLK` | definitions, expanded through `INSERT` | a definition is never drawn on its own |
| `LAYER` table | names, ACI colours, off and frozen flags | `.layers()`; a layer an entity names but the table omits is added |
| `$INSUNITS`, `$EXTMIN`/`$EXTMAX`, `$ACADVER` | `units`/`scale`, `extents`, `version` | |
| `DIMENSION`, `LEADER`, `MLEADER` | **not read**, counted and said once | every reader re-derives these from its own style: explode them to lines and `TEXT` to keep what was drawn |
| `3DSOLID`, `REGION`, `BODY`, `SURFACE`, `MESH`, `3DFACE`, `HELIX` | not read | ACIS SAT data or a mesh: export the part as STEP for `import_step` |
| `ACAD_PROXY_ENTITY`, `OLE2FRAME` | not read | a proxy for an application object: open it in the authoring program and explode it |
| `IMAGE`, `WIPEOUT`, `VIEWPORT`, `UNDERLAY`, `LIGHT` | not read | not drawing geometry |
| `XLINE`, `RAY`, `MLINE`, `TOLERANCE`, `SHAPE` | not read | infinite construction lines and the rest |
| binary DXF, DWG | **refused** before parsing, with the way out | save as *AutoCAD DXF (R12 or later)*, not *Binary DXF*; export a DXF from the CAD program or the ODA File Converter |

Every entity type not read is counted under its DXF name in one info
diagnostic and in the feature's `skipped` arg, so a sheet that lost
something says what.

**Limits.** The reader is ASCII-only and planar: an entity whose
extrusion direction (OCS) is not ±Z is skipped with a warning, a −Z one is
mirrored as the OCS says; a 3D polyline keeps its vertices' z for
`.points()` and `.contours()` and projects onto XY for `.sketch()`. Text
comes back as labels, never as outlines (`text()` sets type). Per-entity
line types, widths and colours are read but nothing draws them. A block
definition cannot be imported on its own: every `INSERT`
is expanded in place, so a furniture symbol comes in where the plan put
it. Layers are the only selector (no handles). What a hostile file cannot
do (`docs/security.md`): the byte-level pre-scan refuses more
than 64 MB, 8 million group-code pairs, 1 million entities (2 million
after `INSERT` expansion — a `MINSERT` array is a multiplier), 10 million
vertices, a line over 1 MB, a bad group code or a pair without its value,
naming the line; a `TEXT` is cut at 4 KB and a `SPLINE` over 10 000 points
skipped, each said; a non-finite or absurd coordinate (past 10¹² mm) makes
the entity malformed, counted, never fatal. The parse runs under the import deadline
(`min(60 s, what is left of the evaluation's budget)`) and a file that
outlives it is a diagnostic.

#### `import_step(source, *, name=None, layer=None, tolerance=None)` → Body | list[Body]

```python
parts = import_step("bearing_608.step", name="bearing")   # the asset's name, or asset("...")
outer = parts[1]
outer.faces(kind=kind.cylinder)                             # classified like any Cadpole body
shaft = import_step(asset("nema17.step"), name="motor", tolerance=0.001)
```

A STEP file (ISO 10303-21; AP203, AP214 and AP242 all parse, the reader is
AP-agnostic) as **exact B-rep bodies**: one body per `MANIFOLD_SOLID_BREP`,
`BREP_WITH_VOIDS` or bare `CLOSED_SHELL`, in the file's order, each the same
`KSolid` every extrude makes — a boolean operand, STEP-exportable, its faces
classified (`plane`, `cylinder`, `cone`, `sphere`, `torus`) by the same
recogniser. `source` is an `asset(...)` or the asset's name — the file under
`./assets` next to the script — and its `kind` must sniff as `"step"`.
Coordinates are scaled by the file's length unit (`SI_UNIT`,
`CONVERSION_BASED_UNIT` inches and feet; none declared → millimetres, with
an info note). `tolerance` (mm) overrides the vertex-matching distance the
file declares (`distance_accuracy_value`, clamped to `[1e-6, 0.1]`).

**Names.** Faces are named by their position in the file's shell,
`<name>/face[k]`, on every body the call made (as an extrude's bodies share
its generator's names): the file's own face order is the closest thing to
construction order a vendor part has, and it survives a re-export where
geometric order does not. A non-empty `ADVANCED_FACE` name that is unique
within its solid becomes an alias, `<name>/<label>`. A face the reader had
to split at a seam (below) is `face[k]` and `face[k]#1`. Bodies are `name`,
`name#1`, … as every multi-output call names them.

**What it does to the geometry, and says so** (each an info diagnostic on
the feature): a full-circle edge on one vertex (what OCCT-based exporters
write for a rim) is split in two; a zero-length edge on one vertex (a pole)
is dropped; a face whose loop walks up one side of a seam edge and down the
other (a vendor's full cylinder) is split into two faces at a meridian
through two of its vertices; an edge written off the surfaces it bounds
(a refitted intersection curve, a few 1e-5 mm off a bore) is lifted onto
both, the way Cadpole's own booleans carry theirs; faces that disagree with
their neighbours are turned to agree; a shell that is inside out is turned
outward. Nothing is healed beyond that.

**What it refuses, naming the entity** (`face[k] (#id): …`): a face whose
surface or curve Cadpole cannot convert — `OFFSET_SURFACE`,
`CURVE_BOUNDED_SURFACE`, `RECTANGULAR_TRIMMED_SURFACE`,
`DEGENERATE_TOROIDAL_SURFACE`, a `HYPERBOLA` or `PARABOLA` edge, an edge
defined only in parameter space (`PCURVE`) — refuses the **whole solid**,
because a shell with a face missing is a hole every later check would call
yours; an `AXIS2_PLACEMENT_3D` whose reference direction is its axis; an
`OPEN_SHELL` alone (a warning) or a `FACETED_BREP` (export it as STL and
use `import_mesh`); a shell that is not closed after conversion; a file
over the caps (`docs/security.md`). Assemblies are not
flattened: every solid comes back where the file put it.

The feature records `asset`, `sha256`, `tolerance`, `importer_version` and
what it read (`solids`, `faces`, `entities`, `unit`, `schema`); the memo
keys the op by the content hash, so a re-evaluation parses nothing.
`examples/vendor_bearing.star` is the worked example.

#### `import_mesh(source, *, name=None, layer=None, repair=repair.report, up=None, units=None)` → Body | list[Body]

```python
scan = import_mesh("knob.stl", name="knob")             # a soup, exactly as the file has it
r = import_report(scan)
if not r.closed:
    fail("the scan has %d open edges" % r.open_edges)
tidy = import_mesh(asset("knob.stl"), repair=repair.weld, name="tidy")
plates = import_mesh("project.3mf", name="plate")       # a Bambu Studio project: one body per print object
room = import_mesh("room.glb", name="room", repair=repair.off)   # glTF: +Y up in metres, turned into Z-up mm
obj = import_mesh("scan.obj", up="y", units="m")        # an OBJ a phone wrote in metres, +Y up
```

An STL (binary or ASCII), OBJ, 3MF or **glTF 2.0** (binary `.glb` or JSON
`.gltf`) file as **mesh bodies**: one body per object — per ASCII-STL
`solid`, per OBJ `o`, per 3MF build item, per glTF node that places a mesh
— named after the file's own name for it (a 3MF `object` name, a Bambu
project's `Metadata/model_settings.config` name, a glTF node's, else its
mesh's), else the asset's stem, else `object[k]`; `import_report(body).object`
says which. The file's groups become the body's named faces,
`<name>/<group>` (an OBJ `g`; a Bambu print-in-place object's parts, so a
set of parts imports as the object the slicer moves as a unit; a glTF
primitive's material), and a file with no groups gives one face,
`<name>/surface`. A 3MF's `<model unit>` scales to millimetres and its
build-item and component transforms are applied; STL and OBJ are read as
millimetres.

**glTF.** The default scene is imported (else the first of `scenes`, else
every root node; a file with meshes and no nodes imports each mesh where it
is defined, with a warning). Node transforms — `matrix`, or `translation`,
`rotation`, `scale` — compose down the hierarchy, and a node that mirrors
has its triangles re-wound so the surface stays outward. A mesh placed by
several nodes is several bodies, each named by its node, else by the mesh,
with `[k]` on every body that shares a name (`chair[0]`, `chair[1]`).
Primitives of mode 4 (triangles, the default) are read, indexed by `u8`,
`u16` or `u32` or not indexed; points, lines, strips and fans are skipped
with a warning that counts them. `POSITION` is `float32` (or an integer
form `KHR_mesh_quantization` allows). Each primitive's material is a
face: `<name>/<material>`, `<name>/material[k]` for a material without a
name, `<name>/default` for a primitive with none; a body with one unnamed
material is `<name>/surface`. A `.gltf`'s buffers are `data:` URIs or
`.bin` files beside it — assets of the document, found by file name (a
path in the `uri` is read for its last segment; a URL is refused, never
fetched), and hashed into the import's memo key so a changed `.bin` is a
new import. Not read, and said: textures (`baseColorTexture` and the rest;
the base colour factor is used), `KHR_materials_unlit` (Cadpole has no unlit
shading; the colours are lit), skins (the bind pose), morph targets (the
base shape), animations, cameras, sparse accessors (the primitive is
skipped). A file that *requires* Draco or meshopt compression is refused
naming the extension; so is glTF 1.0.

**Axes and units.** Cadpole is +Z up in millimetres. glTF is +Y up in metres
by its specification, so a glTF file is turned without being asked:
(x, y, z) → 1000·(x, −z, y) — a quarter turn about +X, the file's +Y
becoming +Z and its +Z (toward a glTF viewer) becoming −Y (toward the front
view's eye), winding kept — and an info diagnostic says so. `up=` and
`units=` say otherwise, for any format: `up="y"` or `"z"` is the axis the
file calls up, `units=` its length unit (`"mm"`, `"cm"`, `"m"`, `"inch"`
or `"in"`, `"ft"`, `"yd"`, `"survey_ft"` — the `dxf_units` members;
`units.m` and friends read the same, `units.ft_in` is refused as a way of
writing lengths rather than a unit). Left out, each is the format's own:
glTF `+Y`/metres, STL and OBJ `+Z`/millimetres, 3MF `+Z` in its
`<model unit>` — which `units=` replaces, with a note. So an OBJ a phone
wrote in metres, +Y up, is `import_mesh("scan.obj", up="y", units="m")`,
and a glTF some tool wrote in Z-up millimetres is `up="z", units="mm"`.
The feature records `up` and `units` when given.

**Colour.** A glTF material's `baseColorFactor`, `metallicFactor`,
`roughnessFactor` and `emissiveFactor` (with `KHR_materials_emissive_strength`),
and the alpha of a `BLEND` material, become the body's material — as
`Body.material(color=, metallic=, roughness=, emissive=, opacity=)` would
set them, from the body's most-used material — and a later
`.material(...)` in the script overrides them. **Per-vertex colour** is
carried on the mesh: glTF `COLOR_0` (`float`, normalised `u8` or `u16`,
RGB or RGBA; alpha not used) multiplied by its primitive's base colour, an
OBJ's `v x y z r g b` (sRGB, 0..1, or 0..255 when a value passes 1; a
vertex without one is white), and — when a glTF body's materials disagree
on colour — each primitive's base colour, so every face shows its own. The
body's base colour is then white. The web viewport and `cadpole render` (both
backends) draw it, blended across each triangle, multiplied by the body's
base colour; `import_report(body).vertex_colors` says whether a body has
it. It follows the body through transforms, copies and instances; a
boolean's result, a mesh cleanup and every export (STL, OBJ, 3MF, STEP)
leave it behind. The wgpu backend and the Apple viewer do not draw it.

**`repair` measures and never mends.** `repair.report` (the default) keeps
the triangles exactly as written — an STL stays a soup — and reports what
`cadpole check`'s solid test found on a welded copy; `repair.weld` merges
coincident vertices (at the mesh kernel's weld distance, then at the
document tolerance if that closes more) — the one repair the mesh kernel
performs itself on the way into a boolean — and turns a closed,
consistently wound, inside-out mesh outward; `repair.off` skips the
pairwise crossing search on a mesh too big to want it, and says what the
census found as information rather than warnings — a mesh read without
repair is reference geometry. The crossing search is indexed (the
renderer's BVH) and skipped above 250 000 triangles whatever `repair`
says, and the report's `pairs_skipped` says when. A hole is never filled
and a fold never removed: a mesh that is not closed renders, exports and
warns, and booleans and `cadpole check` refuse it as they refuse every
non-solid.

The feature records the report as `report[k].*` args and says it as
diagnostics: an info summary, a warning for a mesh that is not closed, not
consistently wound, inside out, self-crossing or folded, each with a
position (information under `repair.off`), and the line *"chord
error: not measurable on an imported mesh — the triangles are the
geometry"*. A glTF body's look is recorded as `look[k].*`, which the memo
replays with the rest.

**A room scan — the recipe.** A phone's LiDAR scan (half a million to
five million triangles, a colour per vertex) is reference geometry: put it
on a ghosted display layer, skip the repair, and draw the model over it
with core operations.

```python
scan_layer = layer("scan", doc="The room as scanned", opacity=0.5)
scan = scan_layer.add(import_mesh("room.glb", name="scan", repair=repair.off))
lo, hi = import_report(scan).bbox          # millimetres, Z up: the room's extent
walls = subtract(box(4240, 3240, 2500, at=(-120, -3120, 0)),
                 box(4000, 3000, 2502, at=(0, -3000, -1)), name="walls")
```

`examples/scan_underlay.star` is the whole of it, on a synthetic room.
In the web UI every evaluation sends the mesh again and the browser
decodes it on its main thread, so a parameter change with a
million-triangle scan loaded takes a second or more. On a phone scan past
32 MB, the native cap above applies.

#### `import_report(body)` → ImportReport

The numbers `import_mesh` measured on `body`, from the feature's recorded
args (so it costs nothing and works on a memo hit); an error on a body
another call made. Attributes: `body`, `format`, `triangles_in`,
`vertices_in`, `triangles`, `vertices`, `groups`, `welded`, `weld_pass`
(0, 1 at the weld distance, 2 at the document tolerance), `collapsed`,
`degenerate`, `shells`, `open_edges`, `non_manifold_edges`,
`flipped_edges`, `crossings`, `overlaps`, `pairs_skipped`, `folds`,
`closed`, `oriented`, `inverted`, `solid`, `genus` (`None` unless closed),
`volume` (signed; `None` unless closed and consistently wound), `area`,
`bbox` (a pair of points, or `None`), `flipped_to_outward`, `chord_error`
(always `None`), `object` (the file's own name for the body: a glTF node
or mesh, a 3MF object, an OBJ `o`; `object[k]` when it gave none),
`vertex_colors` (whether the body carries a colour per vertex) and
`summary`.

#### `text(string, *, font="builtin", size=10, align="left", baseline="alphabetic", spacing=0, line_height=1.2, weight=400, features=None, merge=True, plane=XY, at=(0,0), name=None)` → Sketch

```python
label = text("CADPOLE", size=9, weight=700, align="center", baseline="middle",
             plane=XY.offset(3), name="label")
raised = extrude(label, 0.8, name="raised")
```

The string is **shaped**, not laid out letter by letter: kerning, ligatures
and mark positioning all apply, which is the difference between a label that
looks typeset and one that does not. The call also records its string and
frame: a body extruded straight from the sketch is drawn by a colour
drawing (`look="color"`, `cadpole export --view/--section`) as that string, in
real text, rather than as its glyphs' outlines -- in the bundled face with no
`spacing` or `features`. `size` is the em size in millimetres;
the feature records the face's `cap_height` and the run's `width` for people
who want "10 mm tall letters". `\n` starts a new line, `line_height` is in
ems, `spacing` adds millimetres between glyphs, and `features=["-liga",
"+tnum"]` takes OpenType features in HarfBuzz syntax.

`font=` is `"builtin"` (Inter, bundled, a variable face so `weight=` is
continuous), an `asset("MyFont.ttf")` pinned by hash — the recommended way to
use your own — or `"system:Helvetica"`, which resolves from the machine's
font set, **warns** that the model is no longer reproducible elsewhere, and is
refused outright in the browser. Missing glyphs are reported, not drawn.

Glyph contours go through an unconditional nonzero union in font units before
they become loops, because contours in real fonts overlap and Cadpole nests loops
by containment: without it a heavy `ß` or a `%` comes out with a hole punched
where two strokes cross. The consequence is that glyph outlines arrive as
polygons, flattened to 0.05 % of the em — identical at every size and on every
platform. Loops are named `text_1/glyph[3]/loop[0]`.

### 4.16 Vocabularies

Every closed set of strings a keyword accepts is also a frozen namespace
global, named after the keyword it feeds. **The attribute is the string;
both forms are accepted.** `kind.cylinder` evaluates to `"cylinder"`, so
`faces(kind=kind.cylinder)` and `faces(kind="cylinder")` are the same call.
What the namespace adds is checking where the
name is written: `kind.cylnder` is an error on that line —
`kind.cylnder is not a member of kind (did you mean kind.cylinder?); valid:
plane, cylinder, …` — the same `; valid: …` shape as the string parser's
diagnostic, where the string form fails later, inside the call.

| Namespace | Feeds | Members |
| --- | --- | --- |
| `kind` | `faces(kind=)`, `edges(kind=)` | `plane`, `cylinder`, `cone`, `sphere`, `torus`, `bspline`, `nurbs`, `revolved` (faces); `line`, `bspline`, `nurbs`, `intersection`, `circle`, `arc` (edges) |
| `match` | `loft(match=)` | `index`, `arclen` |
| `cap` | `loft(cap=)` | `plane`, `ruled` |
| `sweep_frame` | `sweep_along(frame=)`, `pattern_along(frame=)` — `frame` itself is the assembly function (§P4) | `rmf`, `frenet`, `fixed` |
| `kernel` | `union`, `subtract`, `intersect`, `split`, `pattern_linear`, `pattern_circular`, `pattern_along`, `pattern_on_surface` (`kernel=`) | `auto`, `brep`, `mesh` |
| `preset` | `Body.material(preset=)` | `pla_matte`, `pla_silk`, `pla_glow`, `petg_glossy`, `petg_clear`, `petg_translucent`, `abs`, `tpu_rubber`, `resin_clear`, `resin_frosted`, `glass`, `acrylic_clear`, `acrylic_frosted`, `brass`, `aluminum`, `steel`, `wood_oak`, `led` |
| `monotone` | `test(monotone=)` | `increasing`, `decreasing` |
| `hand` | `helical`, `herringbone`, `internal` in `@cadpole/gears.star` (`hand=`) | `right`, `left` |
| `join` | `Sketch.offset(join=)` | `round`, `miter`, `bevel` |
| `along` | `explode(along=)` | `x`, `y`, `z`, `radial` |
| `ease` | `animation(ease=)`, `keyframe(ease=)` | `linear`, `smooth`, `ease_in`, `ease_out` (`in` is a Starlark keyword, so the one-sided curves carry the prefix) |
| `flavor` | `export(flavor=)` | `standard`, `bambu` |
| `sheet` | `drawing(sheet=)`, `sheet_set(size=)` | `a4`, `a3`, `a2`, `a1`, `a0`, `arch_a`, `arch_b`, `arch_c`, `arch_d`, `arch_e`, `ansi_a`, `ansi_b`, `letter`, `tabloid` (`letter` is ANSI A and `tabloid` ANSI B, by the names they are bought under; every size is landscape) |
| `repair` | `import_mesh(repair=)` | `report`, `weld`, `off` |
| `dxf_units` | `import_dxf(units=)` and `import_mesh(units=)` | `auto`, `mm`, `cm`, `m`, `inch`, `ft`, `yd`, `survey_ft` (`in` is a Starlark keyword, so the member is `inch` and the string `"in"` still parses; not `units`, which already means a drawing's unit format on `drawing()` and `export_*` — the `units` namespace below — and a CSS unit on `import_svg`) |
| `emboss` | `emboss(kind=)` — **and `emboss(...)` is the function** | `raised`, `cut`, `paint`, `scribe` (`raise` is a Starlark keyword, so the member is `raised`; the string `"raise"` still parses) |
| `wrap` | `wrap(method=)`, `imprint(wrap=)`, `emboss(wrap=)` — **and `wrap(...)` is the function** | `project`, `unroll` |
| `terrain` | `terrain(method=)`, `terrain_from_contours(method=)` — **and `terrain(...)` is the function** | `tin`, `fit` |
| `units` | `drawing(units=)`, `export_svg/dxf/pdf(units=)` — **and `units(...)` declares the document's** (§4.1) | `mm`, `cm`, `m`, `inch`, `ft`, `ft_in` (`in` is a Starlark keyword and `ft-in` is not an identifier; the strings `"in"` and `"ft-in"` still parse) |

```python
side = rod.faces(kind=kind.cylinder)                       # == rod.faces(kind="cylinder")
brim = loft([plain, bosses], match=match.arclen, cap=cap.plane, name="brim")
die = subtract(blank, cutters, kernel=kernel.mesh, name="die")
die.material(preset=preset.pla_silk, color="#c8322f")
test("thicker walls use more material", material, sweep={"wall": [1.2, 2.4]}, monotone=monotone.increasing)
```

`dir(kind)` lists the members, `str(kind)` is `kind: plane | cylinder | …`,
`type(kind)` is `"vocabulary"`, `hasattr(kernel, "mesh")` asks without
failing, and a namespace is immutable. A script variable of the same name
shadows the global, as any Starlark variable does:
`kernel = param("kernel", "auto", …)` in `examples/gridfinity_bin.star` still
works, and `kernel` in that file is then the parameter.

**Callable vocabularies**. `emboss`, `wrap` and `terrain` are the
values of `emboss(kind=)`, `wrap(method=)` and `terrain(method=)` *and* the
functions of those names: the namespace is the function, so the call reads
`emboss(plate, "plate/top", label, 1, kind=emboss.raised)`,
`wrap(label, face, method=wrap.unroll)` and `terrain(shots, method=terrain.fit)`.
Everything above holds for them — `emboss.raised == "raised"`, `dir(emboss)`
lists the members, `type(emboss)` is `"vocabulary"`, a misspelt member is an
error at the site — and `cadpole doc emboss` lists the function's parameters
beside the members. They are named after the function rather than the
keyword because `kind` is already the face class vocabulary. `units` is the fourth:
the namespace of the drawing calls' `units=` is also the declaration
`units(units.ft_in)`, so the vocabulary and the call share the one name a
script already knows. A plain vocabulary is not callable: `kind("cylinder")`
is an error saying so.

Each namespace is built from the same Rust list its parser reads
(`cadpole_core::loft::SectionMatch::ALL`, `Cap::ALL`,
`cadpole_core::kernel::KernelChoice::ALL`, `Monotone::ALL`,
`cadpole_core::sketch2d::Join::ALL`, `cadpole_core::assembly::Along::ALL`,
`cadpole_core::animation::Ease::ALL`,
`cadpole_core::export::Flavor::ALL`, `cadpole_import::mesh::Repair::ALL`,
`cadpole_import::dxf::Unit::ALL`,
`cadpole_core::emboss::Kind::ALL` and `Method::ALL`,
`cadpole_core::terrain::Method::ALL`,
`cadpole_drawing::units::UNIT_MEMBERS` against `UNIT_NAMES`,
`material_preset_names()`, and the face
and edge class lists the selectors check), and a test holds the two equal
in both directions, so a namespace
cannot list a spelling its parser refuses or miss one it accepts. `cadpole doc`
lists every namespace under Types, its members as attributes, so an assistant
finds them without guessing.

Not namespaces, and why: `planar` is an alias of `kind.plane` the selector
still accepts, and a namespace lists each thing once; the render `backend`,
`edges` and `grid` modes are CLI and rpc options no script passes;
entity-reference kinds are never spelled by a script; and
`constrain(kind=)`'s constraint kinds
(§3 ConstrainedSketch) are a different `kind=` and are not in this `kind` —
they would take a namespace of their own.

### 4.17 Animations

An animation is a declaration, like a test: `animation(name, keyframes)`
records what the document's parameters do over time and builds nothing.
`cadpole animate main.star -o out.gif` (or `.apng`, or `--format frames` into a
directory) evaluates the document once per frame with the interpolated
values as parameter overrides — through one kernel-op memo, so a
frame that only poses instances runs zero kernel ops and zero tessellations
after the first — and renders every frame with one camera. The same
operation is the `animate` MCP tool and `cadpole op Animate`; `--list` prints
what a script declares.

#### `animation(name, keyframes, *, fps=30, ease=ease.smooth, loop=False)`

| Arg | Type | Notes |
| --- | --- | --- |
| `name` | str | unique per document; what `--name` selects |
| `keyframes` | list of `keyframe(...)` | in strictly increasing time order; the last one's time is the duration |
| `fps` | number | the frame rate `cadpole animate` renders at unless `--fps` or `--frames` says otherwise |
| `ease` | `ease.linear` / `.smooth` / `.ease_in` / `.ease_out`, or the string (§4.16) | the curve every segment takes unless a keyframe overrides it for the segment that starts there |
| `loop` | bool | the animation repeats: the frame after the last is the first, so the last instant is not rendered twice, and the file loops |

#### `keyframe(t, *, camera=None, ease=None, **params)`

`t` is seconds from the start. Every other keyword is a parameter of the
document — `keyframe(2.5, explode=0, crank_turn=720)` — with a value of the
kind its `param()` default has (number, bool, string or `(x, y, z)`).
`camera=view(az=, el=, fov=, zoom=, pan=)` moves the camera; `ease=` sets
the curve of the segment that *starts* here (CSS's convention).

#### `view(*, az=None, el=None, fov=None, zoom=None, pan=None, eye=None, target=None, layers=None)`

The camera at a keyframe: azimuth in degrees from the front view toward the
right, elevation above the horizon (−90..90) and the vertical field of view
(1..170), each optional — `view(az=50)` turns the camera and leaves its
elevation and lens where the previous keyframe put them. `Camera.preset`'s
`az=,el=` form and the `product` (az 30, el 18, fov 24) and `hero` presets
are the same angles. `zoom` (0.1..10) and `pan=(x, y)` move the fitted
camera without turning it: `zoom=1.15` stands it 15 % closer than the
animation's fit, and `pan=(0.05, 0)` slides it right by a twentieth of the
frame's width (so the subject moves left; `y` is up, a fraction of the
height, and each stays within ±0.45). They start at 1 and `(0, 0)`.
`eye=(x, y, z)` and `target=(x, y, z)` are a
**free camera** in model space instead — a neighbour's window looking at
the house — which stands exactly there with nothing fitted; between two
such keyframes the eye and the target ease point to point, and once a
keyframe has placed the eye, `az`/`el` no longer apply (the hold rule
carries the eye forward); `zoom` and `pan` with `eye=` are refused. The same value is `render_defaults(camera=)`
(§4.12), and `--eye`/`--target` on the command line.

`layers={"envelope": False, "terrain": 0.4}` switches display layers
(§4.13) from this keyframe on: `False` hides, `True` shows, a number in
0..1 shows at that opacity. A visibility switches at the keyframe, as a
bool parameter does; an opacity eases to the next keyframe that names the
layer, as a number does; a layer holds its first keyframed state before
its first mention. A `view(layers=...)` with no `az`/`el`/`fov` moves no
camera. `"other"` is the bodies on no layer unless the script declares a
layer of that name; a name nothing declares is refused at the
`animation(...)` call.

```python
lift = param("explode", 0.0, unit="mm", min=0, max=80)
turn = param("crank_turn", 0.0, unit="deg", min=0, max=1440)
...
explode(lid, (0, 0, lift))
if turn:
    pose(sun, rotate=turn)
    pose(carrier, rotate=turn / 4)

animation("assemble_and_turn", [
    keyframe(0.0, explode=30, camera=view(az=30, el=18), ease=ease.ease_out),
    keyframe(2.0, explode=0),
    keyframe(3.0, crank_turn=0, ease=ease.linear),
    keyframe(9.0, crank_turn=720, camera=view(az=50)),
], fps=30, loop=True)
```

```
$ cadpole animate examples/gear_toy.star --list
assemble_and_turn: 9 s at 30 fps (270 frames, loops), keyframes at 0, 2, 2.5, 3.5, 9 s, moves explode, crank_lift, print_layout, crank_turn and the camera
$ cadpole animate examples/gear_toy.star --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o gear_toy.gif
```

Rules:

- **The hold rule.** A keyframe's state is the previous keyframe's state
  with its own values written over it, so a parameter a keyframe does not
  name holds its previous value there; before its first mention a parameter
  is the document's own value (the default, or the document's override).
  Between two keyframes numbers and vectors ease from the earlier state to
  the later one; a bool or a string switches at the later keyframe. Before
  the first keyframe everything is the first state, after the last
  everything is the last.
- **Only parameters move.** A keyframe may name only the document's
  `param()`s, declared before the `animation(...)` call, with values of the
  declared kind and inside `min`/`max` (and `choices`); anything else, an
  empty keyframe list, a time out of order or negative, an `fps` of zero or
  a camera angle out of range is refused at the call, naming the keyframe.
  `param_overrides` on the request (`--param`) sit under the keyframes'
  values.
- **What a frame costs is what changed.** A parameter that only poses
  instances (`pose`, `explode`) costs a rigid bake per instance body and
  no kernel op; one that reaches into geometry re-runs the ops downstream
  of it and hits the memo for the rest, as a sweep cell does. `--preview`
  evaluates every frame as a drag preview: fuses a keyframe reaches
  are skipped and their operands shown.
- **The camera is fitted once** for the whole animation: the request's
  `--view` preset fitted to the union of the scene bounding boxes at every
  keyframe, so nothing jumps as parts come apart or go back together. A
  keyframe's `view(...)` turns that camera about the same target, easing
  from the request's preset (or the previous keyframe's view) to it, the
  azimuth the short way round and the zoom evenly in distance. The fitted
  distance is the one at which the union box is inside the frame at every
  frame of the camera's path — its angle, lens, zoom and pan there — for
  the aspect ratio being rendered, so a camera that drifts and closes in
  never crops the subject, and `cadpole animate --variant NAME=WxH`
  (docs/views.md) frames a landscape and a portrait of one animation each
  for its own shape.
- **Frames.** One per `1/fps` from 0 to the duration, the end included
  unless the animation loops; `--fps` overrides the rate and `--frames N`
  resamples the whole animation to N frames (a quick look). Never more
  than 4096.
- An animation cannot be declared inside a test, and a name used twice is
  an error. The examples' animations are not run by the test gate (a
  render is not a test); `examples/gear_toy.star` and
  `examples/planetary.star` carry one each.

The files: **APNG** (the default; lossless, every browser plays it, the
first frame is an ordinary PNG a plain viewer shows) writes each frame after
the first as the rectangle that changed, so a part turning on a still
backdrop costs the part's pixels; **GIF** builds one 256-colour palette over
every frame by median cut, dithers each frame (Floyd–Steinberg) and writes
the changed rectangle per frame, looping through `NETSCAPE2.0`; **frames**
is `frame_0000.png` … for ffmpeg (`ffmpeg -framerate 30 -i frame_%04d.png
out.mp4`). Both encoders are Cadpole's own (`cadpole_core::render::anim`) and
deterministic. docs/views.md has the flags and a turntable recipe.

---

## 5. Naming and stability rules

1. **Feature ids.** `name=` if given; it must be non-empty and must not
   contain `:`, `#` or `/`; using the same name twice is an error (`feature
   name "x" used twice`). Otherwise `<kind>_<n>`, numbered per kind in call
   order (`sketch_1`, `extrude_1`, `extrude_2`). Adding a call earlier in the
   script renumbers later unnamed features; renaming variables or adding
   comments never does. Name anything you will refer to again.
2. **Body ids** are `<feature id>#<n>`, `n` counting bodies across the
   document from 0. The second region of an extrude has the body *name*
   `<name>#1`.
3. **Consumed inputs** (booleans, transforms) stay in the tree, hidden. The
   result body carries the first input's material.
4. **Patterns** name copies `<name>_1`, `<name>_2`, ... (the original keeps
   `<name>`); without `name`, copies are `copy_<n>`.
5. **Topological names** are generator-relative and survive transforms
   and booleans (`docs/design.md`).
6. **Selection refs** resolve to `ok`, `moved` (matched by fingerprint, name
   updated) or `stale`; stale refs are kept and flagged, never dropped.
7. **Parameters** are global to the document; the same name declared twice
   is an error. A file loaded as a library may not declare parameters; it
   takes values as arguments.
8. **Determinism.** Given the same scripts, overrides and tolerance, ids,
   names, meshes and diagnostics are byte-identical across platforms (native
   and wasm), which is what golden-image tests rely on.

---

## 6. Output

What a script writes, for either kind of work: mesh and STEP files and
print projects (§6.1), a model printed in pieces (§6.2), dimensioned
drawings (§6.3) and multi-page sheet sets (§6.4). A script records what it
wants exported; the host decides whether and where to write (§1). glTF is
written from the command line (§4.13).

### 6.1 Exports and printing

#### `export(bodies, path, *, tolerance=None, flavor=flavor.standard, plate=None, name=None, extruder=None, printer=None, bed=None, template=None)`

Record that `bodies` (a Body or list) should be written to `path`. The format
comes from the extension: `.stl`, `.obj`, `.3mf` or `.step`. For 2D output — a dimensioned drawing, a cut file — see `export_svg` /
`export_dxf` / `export_pdf` in §6.3; `export(set, "set.pdf")` with a
`sheet_set()` (or one of its sheets) as the first argument writes a drawing
set, every sheet a page (§6.4). The slicer conventions and grouping a
3MF is written with are CLI flags (`cadpole export --slicer`, `--group`),
not keywords; `name=` is the one exception, below.

**Bambu Studio projects.** `flavor=flavor.bambu` writes a
`.3mf` as a Bambu Studio *project* — the Production extension's object
files, named plates, an object per piece with a part per body and each
part's filament, the filament list and colours, per-triangle paint, the
printer, process and filament settings, and a rendered thumbnail per plate
— that Bambu Studio and OrcaSlicer open **as a project**, plates and
filaments intact, and slice. **Several `export()` calls to the same path
accumulate into one file**, one call per print object or plate:

```python
export(core, "gear_toy.3mf", flavor=flavor.bambu, plate="Core (print in place)", name="core", tolerance=chord)
export(lid_print, "gear_toy.3mf", flavor=flavor.bambu, plate="Lid", extruder=2)
export([crank_print, knob_print], "gear_toy.3mf", flavor=flavor.bambu, plate="Crank", name="crank", extruder=[1, 4])
```

A storey kit, one storey to a plate, each storey one object whose parts are
its colour classes on their own filament slots (`examples/storey_kit.star`
is the whole script):

```python
for i, parts in enumerate(storeys):           # parts: [(body, class), ...]
    export([b for b, _ in parts], "kit.3mf", flavor=flavor.bambu,
           plate="Storey %d" % (i + 1), name="Storey %d" % (i + 1),
           extruder=[slot_of(c) for _, c in parts],
           printer="bambu_h2c", template=asset("h2c_7.3mf"))
```

* `plate=` is the plate this call's bodies land on: a number, or a name
  that shows in Bambu's plate tab. Named plates are numbered in order of
  first appearance after every plate so far; a call without `plate=` is
  plate 1. A number leaves lower plates empty rather than renumbering.
* `name=` makes the call's bodies **one object with that name, each body a
  part of it** — how a print-in-place set, or a storey of colour classes,
  arrives so nothing can be moved apart. Without it every body is its own
  object, named after the body. (`name=` on a standard-flavour `.3mf` does
  the same merge: one object with a volume per body, as `cadpole export
  --group part` does.)
* `extruder=` is the filament slot (1-based) the bodies print in — one
  number for all of them, or a list with one per body. Without it a body's
  `material(color=)` picks the slot: distinct colours become slots 1..N in
  order of first use across the whole project, and `paint(..., slot=)` pins
  still apply. A body pinned everywhere it appears claims no slot for its
  own colour, so the toy's lid, crank and knob print in the core's four
  slots and the project lists four filaments; a slot only pins open takes
  the first pinned body's colour. Two pins disagreeing about a slot's
  colour is a warning, and the file is still written. Each part carries
  its slot as its `extruder`; a `paint()`ed face its slot as `paint_color`.
* `printer=` names the machine the plates are laid out for and the pieces
  fitted to — `print_check(printer=)`'s table: `"bambu_h2d"`, `"bambu_h2c"`,
  `"bambu_h2s"`, `"bambu_x1c"`, `"bambu_x1e"`, `"bambu_p1s"`, `"bambu_p2s"`,
  `"bambu_a1"`, `"bambu_a1_mini"`, and short forms (`"h2d"`, `"x1c"`). The
  default is the X1 Carbon: 256 mm square, 250 mm tall, its 18 × 28 mm
  cutter corner kept clear. `bed=(x, y)` or `(x, y, z)` in mm is a custom
  bed instead of the printer's (its corner and nozzle reaches dropped).
* `template=` is **a Bambu Studio project you saved for your printer** —
  `asset("h2d_4.3mf")` or the asset's name, a `.3mf` under `./assets` next to
  the script — and the project carries **its** printer, process and
  filament settings: its start G-code, speeds, temperatures, flush volumes.
  The model's filament slots are mapped onto its filaments in order: slot 1
  takes the template's filament 1, and so on. Its per-filament settings are
  cut to the model's slot count, or extended — a slot past the template's
  takes filament 1's preset, with a warning naming the template — and the
  colours become the model's; a flush volume keeps the template's value
  only where both colours are the template's own, every other pair the
  template's largest. The template's printer lays the plates out, so
  `printer=` and `bed=` give way to it (a `printer=` naming another machine
  is refused at the call). **Save the template with exactly as many
  filaments as the kit uses, loaded in the kit's slot order**, and with the
  process you print with. Without a template the project carries Cadpole's
  minimal settings — the printer's bed, nozzles and stock preset names, and
  the filament colours — which both slicers open and slice, but with *their
  own defaults* for everything else (no start or filament-change G-code,
  default speeds and temperatures): pick your printer, process and filament
  presets before slicing. The export says so in a warning.
* **The plate grid** is Bambu's: `ceil(√N)` plates to a row for N plates,
  `1.2 ×` the bed apart, rows towards −y — two to a row for three or four
  plates, three for five to nine, four for ten to sixteen.
* **Where things land.** Every object is set down on the bed: its lowest
  point goes to z = 0, so a storey modelled at its own height prints from
  its slab. In XY a plate's contents are moved *as a group*, every body
  keeping its position relative to the rest, and centred **where every
  nozzle of the printer reaches** — the 300 mm both of an H2D's or an H2C's
  nozzles reach (X 25–325), the bed for a one-nozzle machine — and moved
  off the X1 and P1 series' cutter corner when there is room.
* **Refused, naming the plate:** a plate whose pieces, as the script
  arranges them, are larger than the bed ("turn it 90 degrees" when that
  would fit; "move them closer together, or give some of them a plate of
  their own" for several pieces), and a piece taller than the printer
  prints, and a plate of several filaments with no room for its prime
  tower (below). Nothing is written. **Warned:** pieces wider than what
  every nozzle reaches (a filament the slicer puts on one nozzle cannot
  print past its reach), a piece taller than the lower nozzle prints,
  pieces that still cover the cutter corner, pieces moved off centre to
  make room for the prime tower.
* **The prime tower** of each plate is placed by Cadpole, never on a piece:
  a square the template's `prime_tower_width` across (at least
  45 mm, for OrcaSlicer's wider tower) with 5 mm clear round it, inside
  what every nozzle reaches, at the front of the longest free run along y
  — a tower grows back from its spot the more it purges. Every column
  beside, between, in front of and behind the plate's pieces is tried.
  When a plate that prints more than one filament has no such spot with
  its pieces centred, the pieces move as a group the least distance that
  opens one, staying where every nozzle reaches and off the cutter corner,
  and a warning says how far; when no move does, the plate is refused,
  naming it, the tower and what would help (a quarter turn when that leaves
  room, one filament, a split, a narrower tower). A plate of one filament
  prints no tower and stays centred. A template that turns the tower off
  (`enable_prime_tower` 0) gets none; a spot a template saved for another
  model is never used. The 5 mm is for brims and OrcaSlicer's wider tower.
* **The version the file claims** is `BambuStudio-02.04.00.70`, or an older
  template's own: OrcaSlicer 2.4.2 refuses 02.05 and later. Two values Bambu
  Studio 2.x saves as `-1` and OrcaSlicer refuses
  (`raft_first_layer_expansion`, `tree_support_wall_count`) are written as
  the `2` and `0` Bambu itself wrote before; a template that prints a raft
  or tree supports is warned about it. OrcaSlicer 2.4.2 cannot slice an
  H2C template's start G-code (it does not know `cooling_filter_enabled`):
  open an H2C project in Bambu Studio.
* `plate=`, `printer=`, `bed=` and `template=` need `flavor=flavor.bambu`;
  the flavour needs a `.3mf` path; a path written by one call as a project
  and by another as a standard 3MF is refused by name. `bed=None` and
  `template=None` are "not given", so a parameter can pass through.
* **The web UI shows the plates.** A document with Bambu exports gets a
  *Plates* toggle beside the view controls: every body drawn where the
  export would put it on its plate, in its filament's colour, on the same
  grid, with a tab per plate and a legend of the slots. It is the export's
  own layout (`Model.plates`, computed by the same function the file is
  written from), placed by transform on the meshes the viewer already has;
  nothing is re-tessellated for it, and the model view is untouched.

The CLI form is
`cadpole export --format 3mf --flavor bambu [--plate N] [--printer NAME]
[--bed X,Y[,Z]] [--template FILE] [--group part]` for the whole document on
one plate; `--template` takes a path to a saved project (added to the
document's assets under its file name) or the name of an asset under
`./assets`. `ExportRequest.template` names the asset, `ExportRequest.bed`
is the custom bed.

#### `plate_images(path, *, size=(1600, 1200), background="transparent", backend="raster")`

A shareable picture of every plate of the Bambu project at `path`, and a
contact sheet of them all. Declare it *after* the project's `export()`
calls, once per project:

```python
export(core, "gear_toy.3mf", flavor=flavor.bambu, plate="Core (print in place)", name="core")
export(lid_print, "gear_toy.3mf", flavor=flavor.bambu, plate="Lid", extruder=2)
plate_images("gear_toy.3mf")
```

`cadpole export --run-exports` then writes, beside `gear_toy.3mf`, one PNG per
plate that holds anything — `gear_toy_plate_1_Core_print_in_place.png`,
`gear_toy_plate_2_Lid.png`, … (`<stem>_plate_<n>.png` for a numbered plate;
a name is reduced to letters, digits and underscores) — and
`gear_toy_plates.png`, the contact sheet: every plate at half size,
`ceil(√N)` to a row as Bambu lays plates out, its number and name under it.

What a plate image shows: the plate's parts **where the project puts
them** — set down, centred, as the file has them — standing on the bed
drawn as a light grey slab the size of the printer's bed (a template's, or
`printer=`'s, or the X1 Carbon's 256 mm square), from a three-quarter view a
little above `iso` on the product presets' 24° lens, under the product
rig, edges drawn, the Khronos PBR Neutral tone curve so a filament's
colour is that colour. **Each part is drawn in the colour of the filament
slot it prints in** — a body pinned to another slot with `extruder=` shows
the colour it will come off the printer in, as Bambu's own thumbnails
do, not its `material(color=)`. **Every part is drawn, opaque, whatever
layer it is on**: a print layout kept on a hidden layer beside the
assembled model (`examples/gear_toy.star`'s `print` layer) still fills its
plate's picture and the thumbnails in the file. The camera is fitted to the
whole bed, so every plate of a project is drawn at one scale and nothing is
clipped.

* `size=(width, height)` in pixels, 16 to 8192 a side; 1600 × 1200 by
  default.
* `background=`: `"transparent"` (the default: every pixel the bed and its
  parts do not cover has alpha 0, so the image drops onto any page), or a
  backdrop from `cadpole render --backdrop` — `"studio"` (a mid grey),
  `"white"`, `"black"`, `"#rrggbb"` — which the contact sheet then stands
  in too.
* `backend=`: `"raster"` (the default; milliseconds, deterministic) or
  `"path"` — the path tracer, for a finished shot with real shadows:
  seconds to minutes per plate.

The project's own embedded thumbnails (`Metadata/plate_N.png`, 512 px,
`iso`, no edges) are unchanged by this: a plate image is a second, larger
render of the **same plate scene** — one function builds the scene both
are drawn from, so the two cannot show different things. It is a
declaration, like `test()` and `animation()`: data on the document, no
feature, nothing rendered until a host runs the script's exports.
Refused at the call: a `path` no `export(..., flavor=flavor.bambu)` has
named yet (declare the images after the exports), a second declaration
for one path, a `size` that is not two whole pixel counts in range, an
unknown `background` or `backend`, and a declaration inside a test.

**The CLI form**, for a script that declares nothing and for the
whole-document export: `cadpole export … --plate-images DIR
[--plate-image-size WxH] [--plate-image-background NAME] [--backend
raster|path]` writes the images under `DIR` for `--out`'s project and, with
`--run-exports`, for every project the script writes; a script's own
`plate_images(...)` keeps its size and background and only the directory
comes from the flag. Without `--plate-images` the declared images go
beside their project under `--out-dir`. The `Export` rpc takes the same as
`plate_images`, `plate_image_width`/`height`, `plate_image_background`
and `plate_image_backend`, and answers with `plate_images` (path, plate,
name, size and PNG bytes per plate) and `plate_sheet` — the sheet is the
operation's image, so `cadpole op Export --out sheet.png` writes it and the
MCP tool hands it back as a picture to look at.

**Tolerance, three levels.** The mesh formats tessellate each body
to a chordal tolerance — the sag in mm no triangle exceeds — and there are
three places to say which, nearest wins:

1. `body.tessellation(tolerance=0.01, angle=8)` on the body itself (§3
   Body): the gear's flanks at 0.01 mm while the housing stays at 0.05.
   `angle=` bounds how far the surface normal may turn across a triangle,
   in degrees; 0 switches it off.
2. `export(bodies, path, tolerance=0.02)` on the call: every body of that
   call that has no setting of its own.
3. The document's: `cadpole export --tolerance`, 0.05 mm by default.

A body's own setting is what *everything* that reads its triangles gets —
render, `print_check`, `volume()`, `cadpole inspect` — not only the export, so
`assert_volume` on a body tessellated at 0.01 is measured at 0.01. A rigid
copy (a pattern member, `copy`, `translate`) shares its source's
tessellation; `tessellation()` on the copy is a warning and does nothing.
STEP is exact — it writes surfaces, not triangles — so none of this applies
to it; the document's `--tolerance` is still written into the file as its
uncertainty measure and used to refit a boolean's intersection edges.
In a multi-body 3MF each object is at its own level.

**The report.** Every export says, per body, how many triangles it became,
at what tolerance and which level that came from, the rung the tessellation
ladder actually used when it had to move, and the **chord error it
measured**: for every triangle, its centroid and its three edge midpoints
are each asked how far they sit from the exact surface — the surface at the
same parametric point bounds that from above, and where the bound could
raise the body's maximum a Newton nearest-point search from that parameter
refines it — and the largest is the body's chord error. On a cylinder that
is the sagitta itself, at or under the tolerance (0.045 mm at 0.05 on a
10 mm cylinder). On a twisted spline band it can exceed the tolerance, which
is the reason it is measured rather than assumed. A mesh body's triangles *are* its geometry,
and a non-uniformly scaled body keeps the mesh `scale` remapped for it,
so both report `not measured`. `cadpole export` prints one line per
body:

```
two.stl  sun#0 "sun": 12340 triangles at 0.01 mm (body), chord error 0.0097 mm on sun/side[0.3]
two.stl  ring#4 "ring": 3180 triangles at 0.05 mm (document), chord error 0.0450 mm on ring/bore
```

`cadpole inspect` has the same numbers as a `chord error` row, and the
`Export` rpc returns them as `ExportBodyReport`s (`bodies` on the response
and on each script export's file).

**The warning.** A print-in-place mechanism is made of gaps of
0.15–0.4 mm, and an STL is inscribed in the exact body: at the default
0.05 mm a curved flank sits inside the true one by up to a third of a
per-flank backlash. So when a document has more than one body and a body's
measured chord error is more than **a third** of the smallest gap between
it and another body, the export raises a *warning* — never an error, the
file is still written — naming the body, the error, the gap, the other body
and the tolerance to pass, spelled as the three levels above. The fraction
is a third for every printer. The gap is `distance(a, b)`'s, measured
between the two meshes, and the message names the two faces it lies
between; because a mesh gap is itself uncertain by both chord errors (a
bore's chords cut into its wall, a pin's into the pin), the rule is judged
against the lower end of that range, which errs towards warning. Bodies
that touch or overlap are not a gap: that is the `fuse` check's finding.
The same finding is a `[chord_error]` row of `print_check` and `cadpole
check`, and fails `assert_printable`, so a model test sees an export that
would eat a gap (§7.1).

The script does not write files: `cadpole export --run-exports` and the UI's
Export button do, under a directory the host chooses; the path's directory part
is ignored and only its file name is used. Two exports whose file names are the
same are therefore refused, naming both paths, rather than one of them silently
replacing the other; so is a path that is nothing but an extension. Multiple
bodies in one STL are concatenated; in 3MF they become separate objects with
names and colours; in STEP they become separate `MANIFOLD_SOLID_BREP`s, each
carrying the body name.

**STEP carries the exact surfaces**: a drilled hole exports as a `CYLINDRICAL_SURFACE` that FreeCAD
can measure, tap and fillet, where the same hole in an STL is a ring of flat
facets. The file is ISO 10303-21 part 21 in the AP214 schema, in millimetres,
and every one is re-read by Cadpole's own structural validator before it is handed
back. It needs **every body on the B-rep path**: a body the mesh kernel made is
triangles and cannot become exact surfaces again, so the export is refused with
a diagnostic naming the body. Use `kernel="brep"` on the booleans of a part
that has to export STEP.

**3MF carries colour.** Each body's `material(color=…)` becomes a filament
slot and every `paint(...)` call becomes per-triangle colour, three ways at
once so every consumer sees it: the core specification's `basematerials` and
per-triangle `p1`, PrusaSlicer's `slic3rpe:mmu_segmentation`, and — behind
`cadpole export --slicer bambu` — Bambu Studio / OrcaSlicer's `paint_color`.
`--group part` merges the bodies into one multi-material object instead of a
plate of separate ones.

#### `print_check(body, *, printer=None, min_wall=None, nozzle=0.4, perimeters=2, layer_height=0.2, material="PLA", max_overhang=45, bed=(256, 256, 256), bed_shape="rectangular", build_direction=(0,0,1), min_gap=0.3, orient=False, max_bridge=10)`

Checks the body's triangles and returns a `PrintReport`; a **list of bodies
is one print** and returns a list of reports, with the checks *between* the
bodies run too (below). **Never raises**:
printing checks are advice, so every finding is also a *warning* diagnostic
with the position to look at, and a script decides for itself what is fatal
(`assert_true(r.ok)`). `r.solid` is the one verdict that is not advice — the
surface itself is or is not the boundary of a solid — and `assert_solid(body)`
asks it without a print profile.

`printer=` names a machine (`"bambu_a1"`, or a value from `printer(...)`) and
supplies its bed, nozzle, layer height and material. **Anything spelled out at
the call site wins over the profile**, so `printer="prusa_mk4", nozzle=0.6` is
an MK4 bed with a 0.6 mm nozzle. With no printer the defaults above apply.

`min_wall` defaults to `nozzle * perimeters` — two 0.4 mm extrusions, 0.8 mm.
`orient=True` also scores build orientations without moving anything.

**A bridge is not an overhang.** A flat downward face held up at
both ends -- a window or door head, a lintel over an opening, the roof of a
cavity -- is printed by a slicer as straight extrusions laid across from one
support to the other, without support, as long as the span is short enough.
So a downward face within 2° of level is checked for a span: the edges of its
flat region are *supported* where the surface across them goes down (a jamb,
a wall under it) and not where it goes up or stops; from each triangle a line
is cast both ways across the supported edges, and the shortest line that ends
on a supported edge at both ends is the span. A span of at most `max_bridge`
mm is a **bridge**: left out of `unsupported_area`, counted in `bridge_area`,
`bridges` and `longest_bridge`, and noted as `[bridged]` information. A
longer span stays an overhang, and the warning says it is a span between
supports longer than `max_bridge` rather than a face over air. A cantilever
held up on one side only is never a bridge.

`max_bridge` defaults to **10 mm**, the value Bambu Studio's projects carry
for "max bridge length". A printer with strong part cooling, tuned, bridges
a good deal further; a script that knows its printer passes its own
`max_bridge=`, and `max_bridge=0` counts every flat downward face as an
overhang. `cadpole check --max-bridge MM` is the same (0 turns it off there
too).

**When the part does not fit the bed**, the diagnostic ends with the
`split(...)` call that would make it fit, and `split_advice` / `split_axis` /
`split_pieces` / `split_at` are the same thing as data.

What it measures, all on the tessellated body (so tolerance matters):

| | how |
| --- | --- |
| solid | `cadpole_core::solid::validate`: closed, consistently oriented and embedded, as the three rows below measure it. `solid` is the verdict; when it is false the report carries an error under check `"solid"` saying why, and `ok` is false |
| watertight / manifold | every edge shared by exactly two triangles walking it in opposite directions; boundary edges chained into rims, each reported with its centre, perimeter and edge count; an edge on three or more triangles is non-manifold (an internal wall between two fused lumps gives four) |
| inverted normals | a closed surface enclosing a negative volume |
| self-intersection | the renderer's BVH for the broad phase, plane-interval triangle-triangle tests for the narrow one; a pair has to reach *past* each other's plane, so a shell resting on a face is not a crossing, and the depth is reported |
| internal faces | coplanar pairs of triangles that share area — a face emitted twice, or the wall two fused parts still share — with the area shared; `internal_faces` counts the pairs |
| shells | `components` is the number of separate closed surfaces (2 for a part with a cavity) and `genus` the surface's genus (0 for a ball, 1 for a ring; −1 when it is not closed) |
| wall thickness | a ray down each triangle's inward normal plus four on an 18° cone, to the far side of the solid; the shortest is the wall |
| overhang | `asin(n · −up)` — 0° is a vertical wall, 90° a flat ceiling — with faces resting on the plate excluded, and flat faces held up at both ends within `max_bridge` counted as bridges instead (above) |
| bed fit | the bounding box in the printer's frame, also tried turned 90° and on the diagonal; a `bed_shape="circular"` plate is tested against its diameter using the footprint's diagonal |
| chord error vs gap | when the document has other visible bodies: the body's measured chord error against a third of its `distance()` gap to the nearest of them, less both chord errors — the export warning of `export` above, as a `[chord_error]` warning here |
| mass | the solid volume times the material's density — an *upper bound*, since infill and perimeters make the real print lighter |

**A list of bodies is one print**: `print_check([sun, p1, p2, p3,
ring, carrier])` runs the five checks on each body and three more between
them, the rules a print-in-place mechanism lives by. A finding between two bodies
lands on the first one's report and, like every finding, on the diagnostics.

| check | rule | severity |
| --- | --- | --- |
| `fuse` | two bodies closer than `min_gap` (the profile's, 0.3 mm), touching, or overlapping come off the printer as one part. Reported with the gap, the position, the two faces (`between pin/side and planet_1/bore/side#0`) and the tessellation tolerance as ±; measured by `distance` | error |
| `layer_gap` | a horizontal gap — a face parallel to the plate over another body's face parallel to the plate, within 2 mm — that is not a whole number of `layer_height`s to ±0.02 mm. The slicer rounds it to the layer grid at its whim, so 0.5 mm at 0.2 mm layers becomes 0.4 or 0.6; the message names both and the nearest | warning |
| `bridge` | a body that does not touch the bed and has nothing under its lowest face within 2 mm prints in the air. Its own overhang check could not see that: a body checked alone takes its underside for the bed | warning |

**Where the numbers come from.** One extrusion is 0.42–0.45 mm wide at a
0.4 mm nozzle and two walls closer than about a third of that fuse; a vertical
bore prints 0.1–0.15 mm undersize and a 30 mm circle is out by ~0.1 mm
radially — so 0.3 mm is the gap that survives both, and `min_gap`'s default.
`min_gap` is a Cadpole number, like the 45° overhang
threshold, not a vendor's; pass `min_gap=` or set it on the printer.

**What is not judged, on purpose.** A bridged gap between two *bodies* that
is whole layers and above `min_gap` gets no finding here: whether it prints
is the span of the upper body's underside, which `max_bridge` judges on the
body that has it (above), and the slicer's own bridge detection. A body whose lowest point is on a curved face has no lowest face
to pair and is not judged. Gaps between faces that are not parallel to the
plate are the `fuse` rule's alone: the slicer rounds heights, not widths.

**`cadpole check` judges each mesh export as one print** — every `export([...],
"x.3mf")` (or `.stl`, `.obj`) of two or more bodies, which is what a slicer
opens — and reports these three checks as **warnings** naming the export,
whatever the rule's own severity: it cannot know whether such an export is
one multi-material part, whose bodies are meant to fuse (a die with its
pips), or a plate of separate parts that must not (a planetary). The
script's `print_check([...])` and `assert_printable([...])` are where the
author states the set, and there the rules keep their severities. A STEP
export is not a print and is not judged.

**An object made with `export(..., name=)` is checked as one piece.**
Its bodies are the parts of one object, and a slicer unions their slices:
a wall standing on a slab is not an overhang, two 0.5 mm plates side by side
are one 1 mm wall, a head resting on two piers is a bridge. So `cadpole check`
fuses the parts of each such object and runs every check on the result, in
one report named for the object: `body_id` is `<path>:<object>`
(`kit.3mf:L0`), `fused_body_ids` lists the parts, and an info under check
`"fused"` says how many. Its parts are not also checked one by one, and
nothing between them is a `fuse` finding.

**The fuse is joined on the grid.** Every part that is an
orthogonal solid -- a box, or anything every face of which is perpendicular
to an axis, which is what `union(weld=)` makes of boxes -- is taken apart
into the boxes it fills, and the boxes of every part are joined in one
exact union, welded at the document's tessellation tolerance or 0.01 mm,
the smaller. A part that is orthogonal only in part (a corbel's slope, a
sliver a boolean's cleanup left slanted) puts its exact core on the grid
too and is unioned whole after, its coordinates within the weld moved onto
the grid's planes (as modelled if that union is not a solid). Parts that
share faces -- a wall of one filament standing on the floor of another --
are one solid there. A fuse whose result is still not a solid is the fuse's failure, never the
piece's: the parts are then checked one by one, with an info saying why. A
print kit that exports each story as one named object of a few hundred
boxes, or a body per filament, therefore gets one report per story, as the
slicer will print it.

`PrintReport` attributes: `ok`, `watertight`, `triangles`, `volume`, `area`,
`holes`, `boundary_edges`, `non_manifold_edges`, `flipped_edges`,
`self_intersections`, `degenerate`, `inverted_normals`, `min_wall`,
`thin_area`, `unsupported_area`, `max_overhang`, `bed_contact_area`,
`bridge_area`, `longest_bridge`, `bridges`, `fits_bed`, `fits_bed_rotated`, `size`, `bed_excess`, `errors`, `warnings`,
`summary`, `profile` (the machine it was judged against), `mass` (grams),
`layers`, `split_advice`, `split_axis`, `split_pieces`, `split_at`, and with
`orient=True` also `orientation`, `orientation_reasoning`,
`orientation_changed`.

#### `orient_for_print(body, *, printer=None, max_overhang=45, bed=(256, 256, 256), bed_shape="rectangular", name=None)`

Returns `body` rotated the way it should print and dropped onto the bed.

Candidates are the six axis directions plus the normals of the model's largest
flat faces. Each is scored `1.0 · (1 − unsupported/total) + 0.6 ·
(bed contact / best) + 0.3 · (1 − height/tallest)`, minus 10 when it does not
fit the build volume; a tie leaves the model exactly as the script built it.
**Simple and explainable, not exhaustive** — it will not find a clever 23°
tilt, and it does not model strength across the layer lines, seam placement or
bridging. The reasoning is an `info` diagnostic and a `reasoning` argument on
the feature; `print_check(body, orient=True).orientation_reasoning` gives the
same sentence without moving anything.

#### `paint(body, *, faces=None, color=None, slot=None, name=None, layer=None, shadow=None)`

Paint faces of `body` a colour, for the viewport and for multi-colour export.
Returns the same body. `faces` is a list of Faces
(`body.faces(normal=(0,0,1))`) or of topological names (`"face:3"`); leave it
out to colour the whole body, which is how a body's filament slot gets pinned.
`color` is `"#rrggbb"` or `(r, g, b)` in 0..1. `layer=` puts the body on a
display layer (§4.13); on its own, with no `color=`, it records nothing but
the tag. A face name that does not exist
is an error at the `paint` call, not silence at export time.

`shadow=False` makes the body guide geometry: something the camera
sees and light does not — a zoning envelope over a house, a setback plane,
a lot line — so it neither shadows nor tints nor blocks light in either
renderer, while its own surface is still lit and shadowed by the real
bodies. It is the same flag as `Body.material(shadow=)` (§4.12), body state
rather than a feature, and it is the whole body's: `paint(body,
shadow=False)` alone sets it and records nothing, `paint(body, color=,
shadow=False)` sets it and paints, and `faces=`, `slot=` or `name=` without
a `color=` is an error.

Filament slots are numbered in order of first use — the bodies in feature
order, then the paint calls — so the same script always exports the same slot
numbering. `slot=` pins one instead, and leaves a gap the automatic numbering
skips over.

`paint` records a feature of kind `paint`; nothing is stored on the body, so
the colour is part of the document, shows in the feature tree with its own
source span, and the renderer and the exporter read it from one place.

#### `split(body, plane=None, *, by=None, kernel=None, keep="both", name=None)`

Cut `body` in two and return the pieces, so a model too big for the bed can be
printed in parts. Cut with a plane —
`split(body, plane(origin=(0, 0, 120), normal=(0, 0, 1)))` — or with another
body's surface — `split(body, by=cutter)`. Pass one or the other, not both.

| Arg | Notes |
| --- | --- |
| `plane` | positional; a `Plane`. The pieces come back **along its normal**, negative side first, so an upward-facing plane gives `lower, upper` |
| `by` | a `Body` to cut with. The piece *inside* it comes first. The cutter is **not consumed**: the same cutter usually cuts several bodies |
| `kernel` | `"auto"` (default), `"brep"` or `"mesh"`, as for `union`. A part that has to export STEP wants `"brep"` |
| `keep` | `"both"` (the default) returns both pieces; `"below"` / `"above"` (a plane, along its normal) or `"inside"` / `"outside"` (`by=`) returns that one piece as a `Body` and makes and consumes the other, so no export writes it |
| pegs | `lower, upper = pegs(*split(body, plane(origin=(0, 0, z))), count=3)` registers the two pieces with pegs and sockets across the cut: `@cadpole/print.star` (§6.2) |

Returns `list[Body]` of two, or one `Body` with `keep=` naming a piece. The input body is consumed. Feature: `split`,
inputs the source feature (and the cutter's, with `by=`), outputs the pieces;
bodies are named `name`, `name#1`, as for a multi-region `extrude`.

Face names (§5): every face that survives keeps the name it had, a face the cut
divided becomes `name#k`, and the new face the cut exposed is `<feature>/cut`
on both pieces.

Diagnostics: `split: pass a plane to cut with, or by=<body>`, `split: pass a
plane or by=, not both`, `split: the plane through (x, y, z) with normal
(x, y, z) does not cross the body (its bounding box is entirely on one side),
so there is nothing to cut`.

**`print_check` writes this call for you**: when a body does not fit the build
volume, `report.split_advice` is the `split(...)` to use and `report.split_at`
is the list of cut positions. `examples/split_for_bed.star` does exactly that.

**Known limitation.** truck cannot boolean a body whose *curved* faces cross
the cut — a square post with a bore through it fails with "this shell is not
oriented and closed" for any cut height. With `kernel="auto"` the split falls
back to the mesh kernel and gives correct pieces, which print but cannot export
STEP; with `kernel="brep"` it is an error.

#### `printer(name)`

A named printer profile: bed size and shape, nozzle, layer height, the support
angle and the material. Pass it to `print_check(body, printer=...)` and
`orient_for_print(body, printer=...)`, or read the numbers off it.
`print_check(body, printer="bambu_a1")` takes the name directly.

| Name | Bed (mm) | Nozzle | Layer | Notes |
| --- | --- | --- | --- | --- |
| `bambu_a1` (`a1`) | 256 × 256 × 256 | 0.4 | 0.2 | |
| `bambu_a1_mini` (`a1_mini`) | 180 × 180 × 180 | 0.4 | 0.2 | |
| `bambu_p1s` (`p1s`) | 256 × 256 × 250 | 0.4 | 0.2 | 18 × 28 cutter corner front left |
| `bambu_p2s` (`p2s`) | 256 × 256 × 256 | 0.4 | 0.2 | from the spec page |
| `bambu_x1c` (`x1c`, `x1`) | 256 × 256 × 250 | 0.4 | 0.2 | 18 × 28 cutter corner front left |
| `bambu_x1e` (`x1e`) | 256 × 256 × 250 | 0.4 | 0.2 | 18 × 28 cutter corner front left |
| `bambu_h2d` (`h2d`) | 350 × 320 × 325 | 2 × 0.4 | 0.2 | left nozzle X 0–325 to 320 mm, right X 25–350 to 325 mm; both: X 25–325 |
| `bambu_h2c` (`h2c`) | 330 × 320 × 325 | 2 × 0.4 | 0.2 | left X 0–325 to 320 mm (1 hotend), right X 25–330 to 325 mm (Vortek, 6 hotends): 7 filaments without a purge; both: X 25–325 |
| `bambu_h2s` (`h2s`) | 340 × 320 × 340 | 0.4 | 0.2 | from the spec page |
| `prusa_mk4` (`mk4`, `mk4s`) | 250 × 210 × 220 | 0.4 | 0.2 | |
| `ender3` (`ender3v2`) | 220 × 220 × 250 | 0.4 | 0.2 | |

A Bambu machine's bed is what **Bambu Studio's own printer profile** lets you
print where that is less than the spec page's build volume — the X1 and P1
series' 250 mm height, which their spec pages footnote — read from projects
Bambu Studio saved; the per-nozzle reach of the H2 machines likewise. A
Bambu Studio project export lays its plates out on this bed and centres a
plate's pieces where every nozzle reaches (`export(..., printer=)`).

Case, dashes, underscores and spaces are not significant, so `"Bambu A1"`,
`"bambu_a1"` and `"a1"` are the same machine. An unknown name is an error
listing the ones there are.

Attributes: `name`, `display_name`, `bed`, `bed_shape`, `nozzle`,
`layer_height`, `min_gap`, `max_overhang`, `material`, `density`, `source`.

**`source` is the point.** Build volume, nozzle and layer height are the
vendor's published numbers and `.source` says which page they came from. The
45° overhang threshold, the two perimeters and the 0.3 mm `min_gap` are **not** vendor numbers —
they are Cadpole's defaults, because every slicer's own threshold differs and
changes between releases.

Materials, for the mass estimate: `PLA` (1.24 g/cm³), `PETG` (1.27), `ABS`
(1.04), `ASA` (1.07), `TPU` (1.21), `PA` (1.14), `PC` (1.20).

### 6.2 Printing in pieces: `@cadpole/print.star`

A building printed one story at a time is a stack of pieces that has to go
back together where it came apart. `load("@cadpole/print.star", "pegs",
"peg_sites", "story_bands", "drop_to_bed", "is_box", "PEG")`:

| Function | What it does |
| --- | --- |
| `pegs(lower, upper, at=None, count=3, z=None, name="pegs", column=None, onto=None, **options)` | Pegs standing on `lower`'s top where it meets `upper`, a socket under `upper` over each. Returns `[lower with pegs, upper with sockets]`, consuming what it changes. The pieces must still stand where the model has them, meeting on a horizontal plane (`z=`, default where `upper` starts; pieces that do not meet are refused, saying so). `at=[(x, y), ...]` places them and checks each fits; without it `count` sites come from `peg_sites`, and fewer than `count` is an error naming how many fit. **Each side may be a list of bodies** -- a piece printed as a body per filament: every test runs on the union of its bodies' material, a peg joins the body under it (or `onto=`, one of `lower` or its index; for a key column, the body of the walls it stands against), a socket is cut from every body of `upper` it reaches, and the return is two lists in the order given, untouched bodies returned as they were. **`column=`** (a side length, at least `size`): a peg with no wall top under it stands on a key column `column` across in a room corner, built from the bottom of `lower` up to the joint against both walls (a hair into each) |
| `peg_sites(lower, upper, count=4, z=None, column=None, **options)` | Up to `count` places a peg fits, as `(x, y)`: the peg's footprint wholly on `lower`'s material just under the plane, the socket's footprint plus `min_wall` all round wholly in `upper`'s just over it, and `upper` still solid over the socket's floor (no break-through); each side a body or a list, judged as the union of its bodies. Candidates are both outlines' corners stepped in by the footprint -- the inside and outside corners of walls -- spread out by farthest-point selection; deterministic. With `column=`, also the room corners: wherever two walls of `lower` meet with wall behind both sides of a `column`-wide square in the corner all along them, at the joint and at mid-height (so not beside a door or in front of a window), the square's centre |
| `story_bands(parts, levels, regions=None, scale=1.0, weld=0.01, sliver=None, pegs_per_joint=0, drop=True, name="story", peg=None, layer=None, thicken=None)` | Cut a list of bodies into horizontal bands at `levels` (model units, lowest first) and make each a printable piece: every part clipped to each band it reaches (a box by arithmetic, anything else by an intersection), a clipped piece thinner than `sliver` (default a hundredth of the thinnest band) dropped, every box narrower than `thicken` (in the scaled units: `0.8` is two perimeters) across x or y widened to it about its centre line -- a mullion, a pane, a baluster that would otherwise not print --, everything multiplied by `scale`, each band joined with `union(weld=)` (`weld=None` leaves the parts, for `export(name=)`), each joint pegged with up to `pegs_per_joint` pegs (`peg=` a dict of `PEG` options; a joint with less room says so and gets what fits), each piece dropped onto the bed and put on `layer`. `regions=[(label, ((x0, y0), (x1, y1)), levels), ...]` cuts the parts whose plan centre is in a rectangle at their own levels, as their own stack. Returns `[(label, body), ...]`: `"main.0"`, `"main.1"`, ..., then each region's |
| `drop_to_bed(body, name=None)` | the body moved so its lowest point is z = 0 |
| `is_box(b, rel=1e-6)` | six faces and the volume of its bounding box: what `union(weld=)` joins on its grid |
| `PEG` | the defaults below |

**The fit, for FDM at a 0.4 mm nozzle** (every one a keyword of `pegs`,
`peg_sites` and `story_bands(peg=)`):

| Option | Default | Why |
| --- | --- | --- |
| `size` | 3.0 mm | the side of a square peg or a pin's diameter; under about six extrusions (2.4 mm) a peg is all perimeter and snaps |
| `height` | 2.0 mm | how far it reaches into the piece above: ten 0.2 mm layers |
| `clearance` | 0.15 mm a side | extrusions are 0.42-0.45 mm wide and a vertical hole prints 0.1-0.15 mm undersize (the numbers behind `min_gap`), so this is a slip fit on a tuned printer; 0.1 is a press fit, 0.25 a loose one |
| `depth_clearance` | 0.3 mm | the socket is this much deeper than the peg is tall, so the pieces sit flush |
| `chamfer` | 0.4 mm | the peg's top 0.4 mm is 0.4 mm narrower a side: a lead-in, stepped so a square peg stays boxes |
| `min_wall` | 0.8 mm | material round every socket: two perimeters |
| `shape` | `"square"` | boxes, which join exactly with a box-built model; `"round"` is a pin with a cone tip |

A peg reaches a hair (a thousandth of its size) down into the piece under
it and a socket a hair out of the piece over it, so no boolean meets two
faces exactly coplanar. At 1:96 a 0.5 ft wall is 1.6 mm, which a 3 mm peg
does not fit: stand the pegs on key columns in the room corners
(`column=3.2`, a foot square at 1:96), or pass `size=1.2, height=1.0,
chamfer=0.2` (a floor plate of 0.5 ft is 1.6 mm, which a 2.3 mm socket would
break through), or thicken the floor in the print. Refused, each by name: a
peg at a place that does not fit (for a column, no room corner for it), pieces
that do not meet, more pegs than there is room for, an option `PEG` does not
have, a shape that is not square or round, a chamfer that leaves nothing, a
column narrower than the peg, an `onto=` that is not one of the lower
bodies, levels that do not go up.

A kit printed as a body per filament pegs a joint in one call, the keys on
columns in the corners and the sockets cut across whichever filaments the
floor above is in:

```python
lower, upper = pegs([walls_l0, floor_l0, frames_l0], [floor_l1, walls_l1], count = 3, z = top,
                    column = 3.2, size = 3.0, height = 1.0, clearance = 0.2,
                    depth_clearance = 0.24, chamfer = 0.24, name = "keys_l0")
```

The whole kit, for a house drawn in feet out of boxes:

```python
load("house.star", "HOUSE")                          # builds the model; nothing else is needed from it
load("@cadpole/print.star", "story_bands", "is_box")

S = 304.8 / 96.0                                     # 1/8" = 1'-0": millimetres per foot
kit = layer("kit", doc="the print pieces")
parts = [b for b in bodies(layer=["walls", "floors"], visible=True) if is_box(b)]
pieces = story_bands(parts, [-10.5, -0.5, 10.0, 19.7, 28.5], scale=S, weld=0.01, layer=kit,
                     pegs_per_joint=3, peg={"size": 1.2, "height": 1.0, "chamfer": 0.2}, name="kit")
for label, piece in pieces:
    print_check(piece, bed=(350, 320, 325), max_bridge=40)
    export(piece, "kit.3mf", flavor=flavor.bambu, plate=label, name=label)
```

`cadpole check kit.star --layer kit` then checks the pieces and nothing of
the model they were cut from, and `cadpole export kit.star --run-exports`
writes the project. `stdlib/print_test.star` holds the tests (pegs on a
two-story building of boxes, their volumes and height; the sites on the
walls, spread out and the same every time; pins at given places; thin
walls that take only a smaller peg; story bands cut, joined, pegged and
on the bed; a region at its own levels, scaled), and `cadpole_script_test`'s
`the_print_library_refuses_what_does_not_fit` the refusals.

### 6.3 Drawings

A *drawing* is 2D line work with figured dimensions on it: a hidden-line
three-view of a part, a section, a plan. It is not a render. Engineering and
architectural drawings are the same pipeline with different defaults.

#### `drawing(bodies=None, *, view="three-view", scale=None, sheet="A3", dimensions=True, hidden="dashed", units=None, title=None, font=None, name=None)`

Project the model and return a `Drawing`.

* `view` — `"three-view"` lays out front, top and right in third-angle
  projection with an isometric in the free corner, fits them to the sheet at a
  standard ratio and figures the overall sizes. A single view name
  (`"front"`, `"back"`, `"top"`, `"bottom"`, `"left"`, `"right"`, `"iso"`,
  `"iso-back"`, `"plan"`) draws just that one on its own sheet.
  `"section:z=40"` — or `x=`/`y=`, with the value in millimetres or in any
  written form the unit parser takes (`40`, `1.5in`, `12'-6"`, `3.65 m`) —
  cuts the model with a plane, hatches the cut face and outlines it heavily.
* `scale` — `"1:2"`, `"2:1"`, or the architectural `1/4" = 1'-0"`. `None`
  fits and snaps to a standard ratio, which the title block then states.
* `sheet` — a member of the `sheet` vocabulary (§4.16) or its string, in
  any case: `sheet.a4`…`sheet.a0`, `sheet.arch_a`…`sheet.arch_e`,
  `sheet.ansi_a`, `sheet.ansi_b`, `sheet.letter`, `sheet.tabloid`.
* `hidden` — `"dashed"` (the default), `"omit"` or `"solid"`. Edges the solid
  hides are found by projecting creases and silhouettes and testing each short
  run against the mesh; an edge that goes behind a boss part way along breaks
  where the boss actually is.
* `units` — `"mm"`, `"cm"`, `"m"`, `"in"`, `"ft"`, or `"ft-in"` for the
  architectural `12'-6"` form, rounded to 1/16" (`units.ft_in` and the other
  members of the `units` vocabulary are the same strings, §4.16). Left out
  or `None`, the document's `units(...)` decides — its fraction included —
  and a document that declared none figures in millimetres. The same
  rule holds for `export_svg`, `export_dxf` and `export_pdf` below, and for a
  sheet set (§6.4).
* `font` — `None` (the bundled face, Inter, which `export_pdf` embeds and
  writes as real text) or `"helvetica"` / `"courier"`: every
  string, dimensions and the title block included, in that standard face,
  which `export_pdf` writes as real text with no font embedded.

With no `bodies`, every visible body is drawn, so hidden-line removal works
*between* parts as well as within one.

A `Drawing` exposes `id`, `items` (how much ink, after groups and dimensions
are expanded), `groups`, `layers`, `scale`, `sheet` and `notes` — the last
being anything the pipeline wants to tell you, such as a dimension it could
not place without overlapping.

#### `export_svg(target, path, *, units=None, stroke=0.25, fill=False)`

Record that `target` — a `Drawing`, a `Sketch`, or a list of sketches — should
be written to `path` as SVG. Coordinates are millimetres and the root says so
(`width="Wmm"`), so the file opens at the right size everywhere and re-imports
through `import_svg` as the same geometry. Circular arcs are written as `A`
commands, so a hole stays a hole. One `<g id="…">` per layer, one
`<path id="…">` per loop, ids equal to Cadpole names.

`fill=True` fills the sketch's regions instead of outlining its loops. That is
also what makes a round trip come back as *loops*: an unfilled closed path
bounds nothing, so it re-imports as a construction outline (which keeps the
same geometry, just not a face).

#### `export_dxf(target, path, *, units=None, stroke=0.25, fill=False)`

The same, as DXF: one layer per drawing layer — for a plan, the standard NCS
names — arcs written as polyline bulges so they stay exact, `$INSUNITS` set to
millimetres, and dimensions exploded into lines, solid arrowheads and `TEXT`
so every reader shows what Cadpole drew. Nothing in the file comes from the clock
or from a random source, so the same drawing exports to the same bytes.

#### `export_pdf(target, path, *, units=None, stroke=0.25, fill=False)`

One vector page, its streams deflated. Every string is real text, selectable and found
by `pdftotext`: in a standard face (Helvetica, Courier -- every string a
sheet set writes, by default) with no font embedded, and in the bundled
face (Inter) with the glyphs the file uses embedded as a subset, at each
weight. A character the face lacks makes that string filled
outlines. A sketch's open paths are written as open strokes.
For several pages, one title column and an index, see §6.4.

All three *record* intent, like `export()`: `cadpole export --run-exports` and the
UI's Export button write the files, never the script. `cadpole export --format
svg|pdf|dxf -o FILE` draws the whole document without a script call, and the
`Drawing` rpc (`cadpole op Drawing`, the MCP tool, `POST /api/ops/Drawing`) gives
the same control from outside.

### 6.4 Sheet sets

A **sheet set** is a numbered set of drawing pages of one paper size — a
permit-style concept set, a shop package — written as **one multi-page
PDF** by one call, `export(set, "set.pdf")`. Every sheet carries the same
title column: the project, its address, a boxed stamp, small print, the
issue rows, the **sheet index, generated from the set** with this sheet
marked, the scale, the sheet's name, and its number large in the corner.
On top of that goes whatever the script places: views of the model —
plans, sections, elevations, axonometrics — cut and projected with their
hidden lines removed and clipped to their viewports, and the annotation a
drawing needs — dimensions in feet and inches with ticks, text, wrapped
notes, tables, north arrows, scale bars, legends, hatches, lines with
weights, dashes and colours, white knock-outs, leaders and pictures.

Two coordinate systems, and the rule is short: **a sheet's methods take
paper millimetres**, x to the right and y up from the sheet's lower-left
corner (`set.area()` is the rectangle inside the frame and clear of the
title column); **a view's methods take model millimetres in the view's own
frame** and go through its scale and placement, so a dimension on a plan is
written between two points of the model and states their true distance.
Line weights, dash lengths, text heights and hatch spacings are paper
millimetres everywhere, whatever the scale, so a 2.5 mm label reads the
same at 1:1 and at 1:96.

```python
set = sheet_set("house_set", size=sheet.arch_d, units="ft-in",
                title_block=title_block(project="A SMALL HOUSE", stamp=["CONCEPT STUDY"]))
x0, y0, x1, y1 = set.area()
plans = set.sheet("A2.0", "FLOOR PLANS", scale="1/4\" = 1'-0\"")
v = plans.plan_view(house, cut=ft(4), at=(x0 + 60, y0 + 220), hatch="solid")
v.dimension((0, 0), (ft(30), 0), offset=-14)     # 30'-0", ticks, 14 mm below the wall
v.text("LIVING", at=(ft(15), ft(16)), align="center", valign="middle")
plans.view_title("1", "PLAN", at=(x0 + 60, y0 + 170))
plans.north_arrow(at=(x1 - 40, y1 - 40))
export(set, "house_set.pdf")                     # every sheet, one page each
```

`examples/house_set.star` is the worked example: a small house on a slope
as four ARCH D sheets from one `export()` — a cover with the project data
and a table of the house at three front setbacks, a site plan with 1 ft
contours, the setback and a north arrow, the two floor plans dimensioned,
and a section down the slope with the grade and the 30 ft height line
beside the street elevation. `docs/demo/house_set.png` is its four pages;
`docs/drawings.md` walks through the pipeline and how a set is laid out.

#### `sheet_set(name, *, size=sheet.arch_d, units=None, scale=None, font="helvetica", margin=10, north=0, title_block=None, text_outlines=False)` → SheetSet

| Arg | Notes |
| --- | --- |
| `name` | the set's name: the PDF's title, and the small print under every sheet number |
| `size` | a member of the `sheet` vocabulary (§4.16) or its string — `sheet.arch_d` (36 × 24 in, the default), `sheet.arch_c`, `sheet.a1`, `sheet.a3`, `sheet.letter`, … — or `(w, h)` in millimetres; landscape as given |
| `units` | how dimensions and scale bars write lengths: `"mm"`, `"cm"`, `"m"`, `"in"`, `"ft"`, or `"ft-in"` (or a `units` member), the architect's `12'-6 3/16"` to the nearest sixteenth with tick marks instead of arrows. Left out or `None`, the document's `units(...)` decides, as it does for `drawing()` — its fraction included, so `units(units.ft_in, fraction=8)` writes eighths — and millimetres without one |
| `scale` | the default scale of every sheet: `"1:48"`, `1/4" = 1'-0"` (and every architect's scale from 3" to 1/32"), or a number of paper mm per model mm |
| `font` | `"helvetica"` (the default: the PDF's text is real text, selectable and searchable), `"courier"`, or `"inter"`, the bundled face, real text with its glyphs embedded |
| `margin` | the frame's inset, mm |
| `north` | degrees clockwise from the page's up to true north, for every north arrow on the set |
| `title_block` | a `title_block(...)`, or a dict of its fields |
| `text_outlines` | every string filled as glyph outlines in every format, for a file that must look identical with no fonts at all |

The set's methods: `sheet(number, name, *, scale=None, north=False)` adds a
sheet — numbered as written (`"G0.0"`, `"A2.1"`), listed in the index in
the order added, `scale` its own (the set's otherwise, and `AS NOTED` in the
title column when neither has one), `north=True` a north arrow in its title
column — and returns it; `sheets()` is the numbers in order; `area()` the
drawing area `(x0, y0, x1, y1)`; `size()` the paper `(w, h)` in mm.

#### `title_block(*, project="", address="", block_lot="", stamp=None, notes=None, issue="", date="", drawn="", checked="", extra=None, column=0)` → TitleBlock

The title column's contents. `stamp` is the boxed lines ("CONCEPT STUDY",
"NOT FOR CONSTRUCTION"; a list, or one string with newlines); `notes` the
small print under it, wrapped to the column; `issue`, `date`, `drawn` and
`checked` the label rows, and `extra` more `(label, value)` rows under
them; `column` the column's width in mm (0 picks one for the paper, 100 mm
on ARCH D). The project's lines and the stamp are set smaller where they
would run past the column; the small print wraps. The set adds the rest —
the index, the scale, the sheet's name and number — on every page.

#### Views: `Sheet.plan_view`, `section_view`, `elevation_view`, `axon_view`, `image_view`

Every view is computed when it is placed, from the bodies' tessellations,
by the `Drawing` pipeline's own hidden-line pass (§6.3) run over all the
bodies together — a wall hides the stair behind it — and placed as a group
with a scale, a position and a viewport outside which nothing is drawn.
They share their placement arguments:

| Arg | Notes |
| --- | --- |
| `at=(x, y)` | the paper point the view frame's `origin=(u, v)` (default `(0, 0)`) lands on |
| `box=(x0, y0, x1, y1)` | instead of `at`: the view's bounds centred in this paper rectangle, which is also its viewport; on a sheet with no scale, the largest standard ratio that fits |
| neither | the view centred on the drawing area |
| `clip=(x0, y0, x1, y1)` | the viewport, paper mm; the drawing area by default |
| `crop=(x0, y0, x1, y1)` | the part of the model the view shows, in model mm of the view's frame: the projection is cut to it — a driveway stops where the plan's interest does — and `box=` and the centring use what is left, as does `bounds`. What the script draws on the view is clipped by the viewport alone, so a dimension or a label may stand outside the crop. A crop that holds none of the view is an error naming both rectangles |
| `scale=` | this view's own scale, over the sheet's |
| `layers=` | the layer each body's lines go on: one name, or a dict of body name to layer name |
| `hidden=` | `"omit"` (the default: hidden lines removed), `"dashed"` or `"solid"` |
| `look=` | `"lines"` (the default: the drafting look above) or `"color"`: the view painted as a section render shows it -- every surface the eye sees filled in its body's colour, nearer over farther, edges as fine lines in a darker shade, the cut filled in the colour of what it cuts; a label the script made with `extrude(text(...), h)` is written as text in the bundled face, the size it is in the model, covered by whatever covers it. `plan_view`, `section_view`, `elevation_view` and `axon_view`; `hatch=` does not apply |
| `name=` | the view's name in the drawing |

| Method | The view and its frame |
| --- | --- |
| `plan_view(bodies, *, cut, hatch="lines", hatch_spacing=None, cut_layer=None, …)` | the bodies cut at height `cut` (model mm) and seen from above: the cut faces outlined heavily and hatched (`hatch=` `"lines"`, `"cross"`, `"dots"`, `"solid"` — poché — `"earth"`, `"concrete"`, `"masonry"` or `"none"`; `hatch_spacing` in paper mm), what is below projected. Frame: the model's own x and y. A cut above everything is a roof plan |
| `section_view(bodies, *, plane, depth=None, beyond=True, hatch="lines", …)` | the bodies cut by `plane` — `"x=<mm>"` (looking toward −x; the frame's x is the model's y), `"y=<mm>"` (looking toward −y; the frame's x is the model's −x), `"z=<mm>"` (looking down), any written length, or a `Plane` (looking against its normal, the plane's own x and y) — the cut faces hatched, and what lies beyond, up to `depth` mm of it, projected behind them; `beyond=False` draws the cut alone. The frame's y is up, the model's z |
| `elevation_view(bodies, *, direction, …)` | the bodies seen from `direction`: `"north"`, `"south"`, `"east"`, `"west"` (the face seen from that side, +y being north), one of `drawing()`'s view names, or an `(x, y, z)` toward the eye. A south elevation's frame is the model's x and z |
| `axon_view(bodies, *, az=45, el=30, …)` | an axonometric from azimuth `az` (degrees from the south view turning toward +x) and elevation `el`, the renderer's own convention, so it matches a render; a picture, so `box=` with no scale fits it |
| `image_view(image, *, at, width=None, height=None)` | a PNG asset (`asset("render.png")` — a `cadpole render` of the model, a photograph) with its lower-left corner at `at`, `width` and/or `height` in mm (one keeps its proportions). The PDF and the SVG carry it; a DXF cannot and leaves it out |

A view (`SheetView`) has `scale` (paper mm per model mm), `at` (where its
frame's origin landed), `clip` and `bounds` (paper rectangles), and draws in
its frame's model coordinates: `point(p)` (the paper point a model point
lands on), `dimension(p1, p2, *, offset=8, units=None, text=None,
size=None)`, `text(...)`, `label(text, *, at, to, size=2)`,
`polyline(points, *, weight=0.25, dash=None, color=None, closed=False,
layer=None, under=False)`, `hatch(region, *, pattern="lines", spacing=None,
angle=None, weight=None, color=None, under=False)`, `fill(region, *,
color=None, under=False)` and `sketch(sketch, *, weight=0.25, dash=None,
color=None, fill=False, layer=None, under=False)` — a Sketch's loops and
paths drawn in the frame: a terrain's `contour_sketch()` on a site plan.
**`under=True`** paints beneath the view's own line work, in call order:
the contours, then the roof's outline in white under the projection, is a
site plan whose contours stop at the house.

#### Annotation: the rest of `Sheet`, in paper millimetres

| Method | Notes |
| --- | --- |
| `text(text, *, at, size=2.5, bold=False, italic=False, rotation=0, align="left", valign="baseline", font=None, color=None, layer=None)` | a run of text; `size` is the cap height; `align` `"left"`/`"center"`/`"right"`, `valign` `"baseline"`/`"middle"`/`"top"`/`"bottom"`; `rotation` degrees counter-clockwise. **Returns its measured width** |
| `note(text, *, at, width=None, size=2.2, line_height=None, align="left", bold=False, font=None, color=None)` | text wrapped to `width` mm at the spaces (or broken only at its own newlines), first baseline at `at`, lines `line_height` apart (1.45 × size). Returns the y of the next free line |
| `table(rows, *, at, col_widths, size=2.2, row_height=None, align=None, bold_rows=None, header=True, font=None)` | a ruled table, top-left at `at`: `rows` are lists of cells (strings, or numbers as `str()` writes them — `format()` for anything else), the first a bold header under a heavier rule; `align` per column; a cell too wide for its column wraps and its row grows. Returns the bottom y |
| `dimension(p1, p2, *, offset=8, units=None, scale=None, text=None, size=None)` | a dimension between two paper points at the sheet's scale: the number is the model distance, in the set's units (ft-in with ticks) or `units=`; the line `offset` mm to the left of `p1 → p2`; `text=` replaces the number |
| `north_arrow(*, at, r=9, angle=None, label=True)` | a ring and a needle turned to the set's `north` (or `angle`), with an N; `label` writes TRUE NORTH under it |
| `scale_bar(*, at, scale=None, length=60)` | alternate filled and open boxes over round feet (imperial) or metres at the sheet's scale, labelled |
| `legend(items, *, at, size=2, title="LEGEND")` | `(swatch, label)` rows; swatches `"line"`, `"heavy"`, `"dashed"`, `"center"`, `"hatch"`, `"cross"`, `"dots"`, `"fill"`, `"box"`, `"ring"`. Returns the bottom y |
| `hatch(region, *, pattern="lines", spacing=None, angle=None, weight=None, color=None, layer=None)` | a region — a list of points or a Sketch, whose loops nest into regions with holes — hatched; `spacing` mm, `angle` degrees |
| `fill(region, *, color=None, layer=None)` | a region filled, black by default; `color="white"` **knocks out** what was drawn before it |
| `polyline(points, *, weight=0.25, dash=None, color=None, closed=False, layer=None)` | a line with a weight in mm; `dash` `"solid"`, `"dashed"`, `"hidden"`, `"center"` (property lines), `"phantom"`, or a list of dash and gap lengths |
| `grade_line(points, *, weight=0.7, hatch_depth=None)` | a heavy line with earth hatch under it to `hatch_depth` mm |
| `rect(lo, hi, *, weight=0.25, fill=None, dash=None)`, `circle(*, center, r, weight=0.25, fill=None)` | stroked, or filled with a colour |
| `label(text, *, at, to, via=None, size=2)` | text at `at` with a leader to an arrow at `to`, through `via` |
| `view_title(number, title, *, at, scale=None, width=140)` | a drawing title: a numbered ring, the title, `Scale: …` (the sheet's, or `scale=`; `""` for none), a rule `width` long |
| `sketch(sketch, *, at=None, scale=1, weight=0.25, dash=None, color=None, fill=False, layer=None)` | a Sketch's loops and paths, its coordinates × `scale` + `at` |

A sheet also answers `number()`, `name()`, `scale()` (as its title column
writes it), `area()` and `items()` (how much ink it holds). Colours are
`"#rrggbb"`, a name (`black`, `white`, `grey`, `lightgrey`, `darkgrey`,
`red`, `blue`, `green`) or `(r, g, b)` in 0..1; everything lands on a named
layer (`A-ANNO-TEXT`, `A-ANNO-DIMS`, `A-ANNO-SYMB`, `M-SECT`, …, or
`layer=`), which the DXF keeps and the SVG writes as groups, and paints in
the order it was drawn.

#### `text_width(text, *, size=2.5, font=None, bold=False)` → mm

The advance width of `text` at cap height `size` in `font` — `"helvetica"`
(the default), `"courier"`, or `"inter"` — the number a script needs to put
the next thing after a label. Helvetica and Courier are measured from
Adobe's own metrics, so what the PDF shows is exactly this wide; Inter by
the shaper that draws it.

#### Writing a set: `export(set, path)`, `export(sheet, path)`

`export(set, "set.pdf")` asks for one PDF of every sheet, a page each at
the paper size; `.svg` or `.dxf` for one file per sheet,
`<stem>_<number>.svg`; `export(sheet, "a2.pdf")` for that sheet alone in any
of the three. Like every `export()` it records intent, and the pages are
drawn and written by the host after evaluation — `cadpole export
--run-exports`, and the `Export` rpc with `run_script_exports` that every
front-end calls — never during an ordinary evaluation, so the viewer does
not deflate a set of ARCH D pages on every keystroke. A set's pages are
written as they stand at the end of the script, sheets added after the
`export()` call included. The body-only keywords (`tolerance=`, `flavor=`,
…) are refused for a set.

`cadpole export plans.star --run-exports --out-dir DIR` with no `--out` writes
the script's own files and nothing else — the `Export` rpc's
`script_exports_only` — so a set's export meshes no body it does not draw
and warns about none. `--out FILE` adds the whole document beside them,
exported and checked.

`cadpole op Drawing --view sheet:A2.0 --format svg` (or `pdf`, `dxf`; the MCP
`Drawing` tool, `POST /api/ops/Drawing`) previews one sheet exactly as the
export draws it, and `--view sheets --format pdf` the whole set.

The PDF is vector throughout, every stream deflated, and its text is text:
a string set in Helvetica or Courier is written in the standard face with
WinAnsi encoding and no font embedded — every PDF reader carries those
faces — so `pdftotext` finds every sheet name, dimension and note. A
character WinAnsi lacks falls back to outlines of the bundled face, per
string.

#### A table of the model at other parameter values: `report(name, value)`, `sweep_table(configs, *, labels=None)`

A cover sheet wants the model's checks at several configurations — the
house at three setbacks, two options side by side — and a script cannot
evaluate itself: an evaluation that re-entered itself would be a recursion
with no bound in time. So the host does it. `report(name, value)` records a
named value of this evaluation (a number, a bool, a string or an `(x, y,
z)`; the last report of a name wins). `sweep_table(configs, labels=)`
returns one dict per configuration — `label`, `params`, `pending`, `ok`,
`error` and `values`, the dict of every `report()` that evaluation made:

```python
report("height", ridge - lowest_grade)
rows = sweep_table([{"front_setback": s} for s in [10.0, 15.0, 25.0]], labels=["10 FT", "15 FT", "25 FT"])
cover.table([["SETBACK", "HEIGHT"]] + [[r["label"], format(r["values"].get("height", 0), ".1f")] for r in rows],
            at=(x0 + 10, y1 - 100), col_widths=[40, 40])
```

Where the rows are worth their cost — `cadpole export --run-exports` (the
`Export` rpc with `run_script_exports`) and the `Drawing` rpc's sheet
preview — the host evaluates the document once to learn what it asks for,
once per configuration with those parameters over the document's own, and
once more with the rows handed in; the evaluations share one kernel memo,
so the last rebuilds nothing the first built. **Everywhere else —
the viewer, `cadpole test`, every evaluation but that last one — every row is
`pending`, with empty `values`**, so read them with `.get()` or a guard, as
the example does. A configuration that fails is a row with `ok=False` and
its first error. `configs` names parameters the document declares; at most
64 configurations per document, each a whole evaluation.

---

## 7. Tests, assistants and diagnostics

A script can state what must stay true of its model (§7.1), check and report
while it is evaluated (§7.2), and every problem comes back as a diagnostic
with a span (§7.3).

### 7.1 Model tests

The script is the model, so its tests live in the script, next to the
geometry they are about. A test is a function registered with `test(...)`.
It runs **after** the whole script has built the model, with every body and
parameter still in scope, and fails by raising — an `assert_*`, `fail()`, or
any script error. An ordinary evaluation (`cadpole eval`, the viewer, a render)
records the names and runs nothing, so a test costs the viewer nothing and
may be as expensive as it likes; `cadpole test main.star` runs them, and
`//src/rust/cadpole-script:cadpole_script_test` runs every test the examples
declare, so an example's tests are CI.

#### `test(name, func, *, sweep=None, monotone=None)`

| Arg | Type | Notes |
| --- | --- | --- |
| `name` | str | unique per document; what the report prints |
| `func` | callable | called with no arguments after the model is built; what it returns is kept when it is a number |
| `sweep` | dict[str, list] | parameter name to the values to try. The test runs once per point of the cartesian product, the document re-evaluated with that point as an override — first parameter slowest, like nested loops. Values take the forms a `param` default does. Capped at 256 points per test |
| `monotone` | `monotone.increasing` / `monotone.decreasing`, or the string (§4.16) | a claim across a one-parameter sweep: `func` returns a number, and it must strictly rise (fall) from point to point |

```python
r_top = param("top_radius", 56, unit="mm", min=20, max=120)
wall = param("wall", 2.4, unit="mm", min=1.2, max=6)
shell = revolve(profile, axis=(0, 0, 1), name="shell")
pot = subtract(shell, cutters, name="pot", kernel="mesh")

def hollow_and_printable():
    assert_solid(pot)
    assert_bbox(pot, (-r_top, -r_top, 0), (r_top, r_top, h), tol=0.6)
    assert_true(pot.volume() < math.pi * r_top * r_top * h, msg="the pot is hollow")
    assert_printable(pot)

test("a hollow, printable pot on its footprint", hollow_and_printable)

def material():
    assert_solid(pot)
    return pot.volume()

test("thicker walls use more material", material, sweep={"wall": [1.2, 2.4, 4.0]}, monotone="increasing")
test("a solid at every top radius", material, sweep={"top_radius": [20, 56, 120]})
```

```
$ cadpole test examples/planter.star
PASS a hollow, printable pot on its footprint
PASS the drainage hole goes through the floor
PASS the floor falls toward the drain
PASS the collar stands proud of the wall
PASS each channel runs from the hole to the edge of the foot  drains=2
PASS each channel runs from the hole to the edge of the foot  drains=4
PASS each channel runs from the hole to the edge of the foot  drains=6
PASS thicker walls use more material  wall=1.2  = 80866.301
PASS thicker walls use more material  wall=2.4  = 112447.55
PASS thicker walls use more material  wall=4  = 153269.192
PASS a solid at every top radius  top_radius=20  = 73617.374
PASS a solid at every top radius  top_radius=56  = 112447.55
PASS a solid at every top radius  top_radius=120  = 179936.849
13 passed, 0 failed
```

What is reported, per test or per sweep point: the name, pass or fail, the
message of the assertion that raised (or the script error), the line it
raised on, the sweep point (`wall=4`) and the number the function returned.
A sweep point at which the model itself does not build is a failure at that
point. `--name` runs one test, `--json` prints the
`RunTestsResponse`, and the exit code is 1 when anything failed or the model
did not build. The same operation is the `run_tests` MCP tool and `cadpole op
RunTests`.

Rules:

- A test declared in a `load()`ed library runs too, after the entry file, in
  declaration order across files.
- `test()` inside a test is an error; so are a name used twice, an empty
  sweep list, `monotone=` on anything but a one-parameter sweep, and a sweep
  over a parameter the document does not declare (which the runner refuses
  by name, since an override nothing declares is silently ignored and the
  test would pass for the wrong reason).
- Geometry a test builds — a probe body, an `intersect` to measure
  interference — belongs to that run and is discarded with it: it never
  reaches the viewer or an export.
- Top-level `assert_*` calls are not tests. They guard every evaluation and
  their failure is an error diagnostic; use them for what must hold for the
  model to be shown at all, and `test()` for what is slow, swept, or
  informative.
- Sweep points are independent evaluations: each has its own deadline, and
  they run in parallel (`--threads`). A point cannot see its neighbours;
  `monotone=` is the one cross-point claim, and the returned value is in the
  report for anything else.

The expectation helpers. Each raises with a message that says what was
measured against what, and the numeric ones share one closeness rule,
`abs(x − y) ≤ max(tol, rel · abs(y))`, so `tol=` (absolute) and `rel=` (a
fraction of the expected value) mean the same thing everywhere:

| Call | Passes when |
| --- | --- |
| `assert_true(cond, *, msg=None)`, `assert_eq(a, b, *, msg=None)` | see §7.2 |
| `assert_close(a, b, *, tol=1e-6, rel=0.0, msg=None)` | the closeness rule holds, element by element for lists and tuples — nested too, so two `bbox()` results compare directly |
| `assert_volume(body, mm3, *, tol=0.0, rel=0.01, msg=None)` | the body's tessellated volume is `mm3` under the rule; the tessellation lies inside a curved surface, so a curved body measures short, more so the smaller its radius: at the default 0.05 mm tolerance a sphere is about 0.3 % short at r=50, 0.6 % at r=20, 1.1 % at r=10, 1.7 % at r=5 and 6 % at r=1, a cylinder 0.6 % at r=3 and 2.5 % at r=2. The 1 % default covers a sphere from about r=20 up and a cylinder from r=3 up; pass a wider `rel=` for smaller radii (`rel=0.02` for a 5 to 10 mm ball). A polyhedron can take `rel=0, tol=1e-6` |
| `assert_bbox(body, lo, hi, *, tol=0.1, msg=None)` | every coordinate of the bounding box is within `tol` mm; the default is a little wider than the tessellation tolerance a curved extent can fall short by |
| `assert_solid(body, *, msg=None)` | the tessellation is a solid (§7.2) |
| `assert_printable(body, *, printer=None, …, msg=None)` | `print_check(body, …)` with the same keywords has nothing to report — warnings included, because a test wants a verdict where `print_check` gives advice. Nothing is recorded as a diagnostic. `body` may be a list: one print, checked between the bodies too |

There is no separate helper for these: a face by name is
`body.face("plate/top")` (an error when there is none), a face count is
`len(body.faces())`, a report to inspect is `print_check(...)`.

### 7.2 Assistant hooks and diagnostics

| Call | Notes |
| --- | --- |
| `selection()` | list[Ref]; empty when nothing is selected. Scripts can branch on it (`if selection(): ...`) but usually the assistant reads it via `cadpole selection` and edits the script instead |
| `warn(message)` | warning diagnostic at the call site |
| `fail(message)` | Starlark builtin: error diagnostic, stops evaluation |
| `assert_eq(a, b, *, msg=None)`, `assert_true(cond, *, msg=None)` | error with both values on failure |
| `assert_solid(body, *, msg=None)` | error naming every failing check when the body's tessellation is not a solid: holes, non-manifold edges, flipped faces, inside out, triangles passing through each other, doubled or internal faces (`cadpole_core::solid`). The short form of `assert_true(print_check(body).solid)`. A solid that carries a *fold* (two adjacent triangles facing opposite ways) passes, with a warning saying where: a fold is not a hole, but the mesh kernel's next boolean on that body may lose its face names over it; `cadpole inspect` shows it |
| `assert_close(a, b, *, tol=1e-6, rel=0.0, msg=None)` | numbers, or lists/tuples of numbers element by element; passes on `abs(a − b) ≤ max(tol, rel·abs(b))` — §7.1 |
| `assert_volume(body, mm3, *, tol=0.0, rel=0.01, msg=None)` | the tessellated volume against `mm3`, same closeness rule; the message carries both numbers — §7.1 |
| `assert_bbox(body, lo, hi, *, tol=0.1, msg=None)` | every corner coordinate within `tol` mm — §7.1 |
| `assert_printable(body, *, printer=None, min_wall=None, nozzle=None, perimeters=None, layer_height=None, material=None, max_overhang=None, bed=None, bed_shape=None, min_gap=None, msg=None, max_bridge=10)` | `print_check` with the same keywords has nothing to report, warnings included — the chord-error-vs-gap warning of §6.1 among them, a bridge within `max_bridge` not (it is information, not a finding); records no diagnostic — §7.1. A list of bodies is one print: the checks between them (`fuse`, `layer_gap`, `bridge`, §6.1) run too, so `assert_printable([sun, ring, carrier])` is the planetary's fit test |
| `print(...)` | Starlark builtin: each line becomes an info diagnostic in `Model.diagnostics` |
| `test(name, func, *, sweep=None, monotone=None)` | registers a model test; runs after the model is built, only under `cadpole test` and the example gate — §7.1 |

### 7.3 Diagnostics reference

| Severity | Effect | Examples |
| --- | --- | --- |
| `error` | evaluation stops at the call; the model contains everything before it; `cadpole eval` exits 1 | `extrude distance must be non-zero`; `feature name "lid" used twice`; `union failed (...)`; `Variable `boxx` not found`; `rect: w and h must be positive (got -1, 5)`; `conflicting constraints: "top width" contradicts "width" by 5 mm` |
| `warning` | recorded, evaluation continues | override type mismatch; `warn()`; `redundant constraint: "top width" is already implied by "width"`; a constrained chain that does not close; print checks |
| `info` | recorded | boolean kernel routing (§4.5: over the face budget, or the B-rep kernel failed and the mesh kernel took over); `sketch "plate" is under-constrained: 3 degrees of freedom remain (free: b, top)`; `print()` output |

Every diagnostic carries `span = {path, start_line, start_col, end_line,
end_col}` (1-based) when it comes from a script location, and `feature_id`
when it belongs to a feature. Kernel failures are attributed to the call that
triggered them, with the operation name and inputs in the message.

Message shape: `<what failed>: <why> (<values>)`, lower-case, no trailing
period, the offending values quoted or in parentheses so an assistant can
locate them in the script.

`cadpole eval` exit codes: `0` ok, `1` errors in the document, `2` usage/input
error. `cadpole render`/`sweep` still write an image on `1` (with the failing
tiles labelled) so partial results are visible.

---

## 8. Worked examples

### 8.1 Planter with drainage and a saucer

```python
# planter.star — a tapered pot with a drainage hole and a matching saucer.
h = param("height", 90, unit="mm", min=40, max=200)
r_top = param("top_radius", 55, unit="mm", min=20, max=120)
r_bottom = param("bottom_radius", 40, unit="mm", min=15, max=100)
wall = param("wall", 2.4, unit="mm", min=1.2, max=6, doc="3 perimeters at 0.8")
base = param("base", 4, unit="mm", min=2, max=10)
drain = param("drain_radius", 4, unit="mm", min=0.5, max=15)

slope = (r_top - r_bottom) / h
profile = sketch(XZ, name="profile").polygon([
    (drain, 0), (r_bottom, 0), (r_top, h), (r_top - wall, h),
    (r_bottom - wall + slope * base, base), (drain, base),
])
pot = revolve(profile, name="pot").material(color=(0.82, 0.55, 0.42), roughness=0.85)

# Saucer: a shallow dish 6 mm wider than the pot's foot, placed beside it.
lip = 6
saucer_profile = sketch(XZ, name="saucer_profile").polygon([
    (0.5, 0), (r_bottom + lip, 0), (r_bottom + lip, 8), (r_bottom + lip - 2, 8),
    (r_bottom + lip - 2, 2.4), (0.5, 2.4),
])
saucer = revolve(saucer_profile, name="saucer").translate((2 * r_top + 20, 0, 0))

export([pot, saucer], "planter.stl")
```

To compare wall thickness against top radius in one image: `cadpole sweep examples/planter.star --param
wall=1.6,2.0,2.4 --param top_radius=50,55,60 --view front -o wall_study.png`.

### 8.2 Bracket with fillets and a mirrored twin

```python
w = param("width", 60, min=30, max=120)
d = param("depth", 30, min=20, max=80)
t = param("thickness", 6, min=3, max=12)
bolt = param("bolt_d", 5.5, min=3, max=10)

plate = box(w, d, t, center=True, name="plate")
boss = cylinder(8, 11, at=(0, 0, t - 1), name="boss")          # overlaps 1 mm: no coplanar faces
body = union(plate, boss, name="body")
hole = cylinder(bolt / 2, t + 2, at=(-w / 2 + 8, 0, -1), name="hole")
holes = pattern_linear(hole, 2, (w - 16, 0, 0), name="holes")
bracket = subtract(body, holes, name="bracket")  # a list: a keyword after *holes is a parse error

# Exact: the plate's four rim edges become cylinders meeting
# in elliptical mitres, the boss root a concave torus and the hole mouths
# convex ones, all wired into the B-rep; see examples/filleted_bracket.star.
top = bracket.faces(normal=(0, 0, 1), on_plane=XY.offset(t)).one()
bracket = fillet(bracket, edges=top.edges, radius=2, name="rounded")

# Or pick the edges by what they are, which survives a change to the plate:
# bracket = fillet(bracket, edges=bracket.edges(direction=(0, 0, 1), convex=True), radius=3)
# A mirrored twin for the other side of the enclosure: the image
# alone, every face still called what the bracket called it.
twin = mirror(bracket, plane=YZ.offset(w / 2 + 5), keep=False, name="twin")
```

A part that must export STEP should keep its corners on the B-rep path:
`fillet2d(profile, r)` (§4.3) or a `rounded_rect` profile, both exact.

### 8.3 Box with a lid

```python
inner = param("inner", 40)
wall = 2.4
outer = inner + 2 * wall

base = subtract(box(outer, outer, 30, center=True, name="base_outer"),
                box(inner, inner, 30, center=True, at=(0, 0, wall), name="base_cavity"), name="base")
lid = box(outer, outer, 4, center=True, at=(0, 0, 31), name="lid")     # 1 mm gap
```

### 8.4 Organic vase with noise displacement (field bodies)

```python
h = param("height", 140)
r = param("radius", 45)
f = field.cylinder(r, h, round=6)                                    # soft column
f = f.taper(-h / 2, h / 2, bottom=1.0, top=0.7).twist(0.6)
f = f.displace(field.noise(scale=10, amplitude=1.5, seed=7))
f = f.shell(2.0).subtract(field.box(2 * r, 2 * r, 4).translate((0, 0, h / 2)))   # open top
vase = mesh(f, resolution=0.5, name="vase")
vase.material(color="#9fb7a4", roughness=0.7)
export(vase, "vase.stl")
```

### 8.5 A dimensioned plate (constraints)

The literal way to put a hole 10 mm in from the right-hand edge of a `w`-wide
plate is `(w / 2 - 10, 0)`: correct, and wrong again the moment the plate stops
being centred or starts being trapezoidal. The dimensioned way says what the
drawing says. Full script: `examples/constrained_bracket.star`; the L-profile
with a filleted inner corner is `examples/constrained_lprofile.star`.

```python
w = param("width", 80)
margin = param("edge_margin", 10)

s = constrained_sketch(XY, name="plate_profile")
bl = s.point(0, 0, name="bl", fix=True)
br = s.point(w, 1, name="br")
tr = s.point(w + 1, 40, name="tr")
tl = s.point(-1, 39, name="tl")
bottom = s.line(bl, br, name="bottom")
right = s.line(br, tr, name="right")
top = s.line(tr, tl, name="top")
left = s.line(tl, bl, name="left")
s.constrain("horizontal", bottom)
s.constrain("horizontal", top)
s.constrain("vertical", left)
s.constrain("vertical", right)
s.constrain("distance", bl, br, w, name="width")
s.constrain("distance", br, tr, 40, name="depth")

# A centreline that is never material, and a hole measured from an edge.
ml = s.point(0, 20, name="mid_left", construction=True)
mr = s.point(w, 20, name="mid_right", construction=True)
axis = s.line(ml, mr, name="centreline", construction=True)
s.constrain("midpoint", ml, left)
s.constrain("midpoint", mr, right)

hr = s.point(w - margin, 20, name="hole_right_center")
hole = s.circle(hr, 2.75, name="hole_right")
s.constrain("on_curve", hr, axis)
s.constrain("distance", hr, right, margin, name="right hole from the right edge")
s.constrain("radius", hole, 2.75, name="bolt")

assert_eq(s.dof(), 0)           # fully dimensioned; `dof()` solves
plate = extrude(s, 6, name="plate")
```
