# Plating: from a model to print-ready plates

A model is usually written the way it is used: the lid on the base, the
wheels on the axles, the figure on its plinth. A printer wants something
else: every piece standing on the bed the way it prints best, apart from
the others, on as few plates as will hold them, with the filaments each
plate needs. Doing that by hand -- a hidden `print` layer of rotated and
translated copies, a plate name per `export()` call, slot numbers picked
per body -- is what every printable example used to do, and what a script
forgets when it exports the assembled model instead.

`print_plan(...)` (`docs/starlark-api.md` §6.5) and `cadpole plate` do it
from the model as written. This note records how and why.

## Shape of the feature

```
items ──► pieces ──► orientation ──► plates (packing, colour) ──► export specs ──► Bambu project
          (what moves    (per piece,      (raster bottom-left fill,     (the existing writer:
           as one)        print-check      AMS slots, variants,           plate grid, prime tower,
                          cost)            coupons first)                 slots, thumbnails)
                               └──────────── one report: every decision with its numbers ─┘
```

* **The planner is `cadpole_print::plate`**: meshes in, a plan out. It knows
  nothing of documents or Starlark, is deterministic (stable orders, ties
  broken by name, no hashing, no randomness), and is unit-tested on
  synthetic meshes.
* **The glue is `cadpole_script::plating`**: it reads bodies, colours and
  paint from a `Context`, calls the planner, makes the placed copies on a
  hidden `print` layer (copying each body's paint onto its copy), and
  records ordinary `export(..., flavor=flavor.bambu, plate=...)` specs. Both
  the `print_plan()` builtin and the engine's `Plate` operation use it, so
  the CLI, MCP and a script get one answer.
* **The file is written by the existing Bambu project writer.** Plates,
  filament slots, the prime tower, the plate grid, thumbnails and plate
  images are already right there; plating only decides *what goes where*.
  The writer centres each plate's contents as a group and places the
  prime tower, so the planner hands it relative positions that leave room
  for both.

## Pieces: what moves as one

A *piece* is a set of bodies that is turned and placed as one rigid unit.
An item the script gives as a list (`[shell, stripes, eyes]`) is one piece,
always. Loose bodies are joined into pieces by looking at each pair whose
bounding boxes come within `joint_gap` (1 mm by default):

| relation | test | result |
| --- | --- | --- |
| overlap | `clearance::distance` reports shared material | **fused**: one piece, printed as one multi-material object |
| skin or inlay | they touch, and at least 10% of the smaller body's surface lies on the other (a ray cast a hair along each triangle's normal finds the other body) | **fused** |
| print-in-place | the gap is under `joint_gap` and the pair is **captive**: neither can be slid away from the other in any of 26 directions (the axes, face and corner diagonals) without colliding | **print-in-place**: one piece, never pulled apart or turned relative to each other |
| anything else | a lid resting on its base, a wheel beside a car | separate pieces |

Captivity is the non-directional blocking test from assembly planning:
body A is free to leave B along `d` when no ray from A's sample points
(vertices and triangle centroids, thinned to a few thousand) along `d`
meets B and no ray from B's along `-d` meets A. A lid lifts off along +z,
so it is free; a wheel on an axle with a hub cap is blocked radially by
the axle and axially by the cap, so it is captive; a planet between the
carrier plates and the ring is blocked every way. Vertex rays can miss an
edge-on-edge collision, so the test leans towards *free*; the cost of that
is two parts printed apart that a script can join with a list.

**Rejected:** a pure clearance threshold (it fuses every lid to its base,
since a modelled lid sits on it); connected components of "touching"
(same); requiring the script to declare every group (`cadpole plate` must
work on a script that never heard of plating); colour as the signal
(a two-colour lid and base are a common, separate pair).

## Orientation

Each piece's candidates are: as modelled, upside down, the four sides,
and the outward normals of its eight largest flat regions -- the same
list `orient_for_print` uses, so each has a name a person recognises
("the 1200 mm² flat face"). Each is scored by a **cost in mm² of support
equivalent**, absolute rather than normalised against the other
candidates, so a number in the report means the same thing for every
part:

```
cost = support_area                         (what print_check's support rule still needs supported)
     + 60 per island + 20 per mm it hangs   (a first layer printed on nothing)
     + 40 per mm a ledge sticks out past max_cantilever
     + 50 per wide round horizontal hole
     + 150 when height over √(first layer) passes max_height_to_base
     + 400 when a declared strength axis (`along=`) is across the layers
     + 0.05 per mm of height                (print time; a tiebreak)
     + 0.5  for any turn at all              (a turn nobody needed is a cost)
     + 10⁶  when it does not fit the build volume
```

Support area is the dominant unit because support material is what ruins
a surface. Evaluating the full support rules (islands, ledges, holes,
first layer) costs a weld and a sweep per candidate, so it is done in two
stages: every candidate's raw overhang area first (a pass over the
triangles), then the full rules on the best four and on "as modelled",
which is always reported as the baseline.

**Strength.** A printed part is weakest across its layer lines, so a snap
arm bent across them breaks at a fraction of the strain it takes along
them (`snap_fit` in `@cadpole/print.star` gives the strain). A piece may
say `along=(x, y, z)`: the model direction its fibres must run in, a snap
arm's length. A candidate that leaves it more than 30° out of the bed
plane pays the strength penalty, and the report says "turned so the clip
arm runs along the layers".

**Print-in-place pieces keep their modelled orientation**: their gaps are
designed on the layer grid (`layer_gap`) and their resting faces two
layers apart, and turning them breaks both. A script can still pass
`down=` for one.

**What a script pins**: `down=(x, y, z)` (that model direction faces the
bed), `orient="keep"` (as modelled), `spin=` an angle about z or `False`,
`plate=` a plate number of its variant, `group=` (pieces that share a plate
when they fit), `coupon=True`. Pinned choices are reported as pinned.

**Rejected:** a continuous search over rotations (it finds a 23° tilt
nobody can explain or reproduce, and tilts print badly on flat-faced
parts); the normalised weighted score of `orient_for_print` (a score
relative to the other candidates cannot be compared between parts or
explained in mm²); scoring with the whole `print_check` (wall thickness
and manifoldness do not change with orientation and are the expensive
half).

## Orientation and packing together

Print quality comes before plate count: a plate saved is never bought
with support. So the optimisation is lexicographic. Every orientation
within 2 mm² and 2% of a piece's best cost, with no extra island, is an
*equally good* print, and those, times the spins about z, are the
alternatives the packer may use. The packer places a piece in its best
orientation when it fits; when it does not fit the plate being filled, it
tries the alternatives before opening a new plate. The report says when
an alternative was used ("laid on its side, the same 0 mm² of support,
so it fits plate 1").

## Packing

Plates are filled by **raster bottom-left fill**. Each piece's footprint
is its triangles projected onto the bed, rasterised at 1 mm and dilated
by half the `spacing` (4 mm between pieces by default); the plate is a
bitmap of where every nozzle reaches (the H2D's and H2C's X 25-325, the
whole bed for a one-nozzle machine), with the excluded corners (the X1 and
P1 cutter) marked taken. A piece goes to the lowest, then leftmost,
position where its bitmap is clear; the spins tried are 0°, 90°, 180°,
270° and the angle of the footprint's minimum-area rectangle. Bitmap rows
are 64-bit words, so a test is a few word ANDs per row.

An L-shaped base and a flat lid nest into each other's bounding boxes,
which a shelf packer of rectangles cannot do, and a raster has none of
the numerical fragility of no-fit-polygon nesting. Randomised search
(annealing, genetic) was rejected: the same script must give the same
file.

**Order:** test coupons first, on their own plate ("Test coupons"), since
they are printed before the rest; then groups, larger first; then pieces
by footprint area, largest first, name as the tiebreak.

**Which plate:** the open plates are tried in order of *colour affinity*
-- fewest new filaments first, then most filaments shared, then plate
number -- and a piece only joins a plate that keeps the plate within
`ams` filaments. A piece that brings a second filament to a plate needs a
prime tower: the plate keeps a free column one tower wide (the writer's
tower plus its margin, 55 mm) at its right side, so the writer always
finds a spot.

## Colour

A piece's filaments are its bodies' colours and their paint, in the
order the project assigns slots (first use). The report counts, per plate,
the filaments and an **estimate of colour changes**: the plate is cut into
layers, each layer's set of filaments found from which bodies (and which
painted triangles) span it, and a layer with *k* filaments costs *k − 1*
changes, plus one where a layer's first filament is not the one the
previous layer ended on. It is a proxy -- the slicer orders filaments
itself and an H2C with a rack of hotends swaps without purging -- but it
ranks layouts the right way and points at the plate that will take all
night.

## Variants

`variants=["colour", "mono", ...]` makes several plate sets in one
project: `"colour"` (each body in its own filament), `"mono"` (every body
on filament 1, packed with no colour constraint and no prime tower) --
each a full set of plates named after the variant ("Colour 1", "Mono 1").
Coupons are printed once, on "Test coupons", before either.

## The report

Every decision is a line a person or an agent can act on:

```
plate 1 "Colour 1" (bambu_h2c, 300 x 320 mm reach): 3 pieces, 2 filaments, ~410 colour changes
  base: as modelled; 0 mm² support (best of 7)
  lid: upside down (on the 2140 mm² flat face): 0 mm² support vs 340 mm² as modelled
  clip coupon: turned so the clip arm runs along the layers (along= (1, 0, 0))
  gear set (sun, 3 planets, ring, carrier): print-in-place (captive within 0.35 mm), kept as modelled
```

It is returned as text and as data (`plan["pieces"]`, `plan["plates"]`),
printed by `cadpole plate`, and returned by the `Plate` operation.

## Follow-ups

* Automatic detection of snap arms (a cantilever whose root is much
  thinner than its body) to set `along=` without the script saying so.
* Packing by true outline at finer resolution near tight fits, and
  nesting small pieces inside a large piece's cavities.
* Per-plate layer-height variants, and orienting for the visible surface
  (seam and top-surface placement).
* Splitting a piece that fits no plate, with `split()` and `pegs()`.
* The web UI's *Plates* toggle showing the plan's report beside each plate.
