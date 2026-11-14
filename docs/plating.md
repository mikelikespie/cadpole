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
items ──► pieces ──► orientation ──► plates ──► layout ──► check ──► export specs ──► Bambu project
          (what moves    (per piece,      (which plate:   (true outlines,   (gap, reach,     (the existing writer:
           as one)        print-check      plates against   out from the      keep-outs on     plate grid, slots,
                          cost)            colour changes)  bed's middle)     the triangles)   thumbnails)
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
  A plate the planner laid out is written exactly as planned -- the
  pieces where they stand, the prime tower in the corner the planner
  kept -- where a plate a script arranged by hand is centred as a group
  and given a tower spot by the writer.

## Pieces: what moves as one

A *piece* is a set of bodies that is turned and placed as one rigid unit.
An item the script gives as a list (`[shell, stripes, eyes]`) is one piece,
always. Loose bodies are joined into pieces by looking at each pair whose
bounding boxes come within `joint_gap` (1 mm by default):

| relation | test | result |
| --- | --- | --- |
| overlap | `clearance::distance` reports shared material | **fused**: one piece, printed as one multi-material object |
| skin or inlay | they touch, at least 10% of the smaller body's surface lies on the other (a ray cast a hair along each triangle's normal finds the other body), and the smaller is *thin* there: its volume over the contact area under a quarter of its size | **fused** |
| print-in-place | the gap is under `joint_gap`, at least half of `min_gap`, and the pair is **captive**: neither can be slid away from the other in any of 26 directions (the axes, face and corner diagonals) without colliding | **print-in-place**: one piece, never pulled apart or turned relative to each other |
| resting | one is printed on the other across a gap of half `min_gap` to two layers, over at least a fifth of its underside: a stand one layer under a chassis, holding it up while it prints | **print-in-place**, kept as modelled |
| held but tight | captive, but under half of `min_gap` clear: a nut on its screw, a lid snapped onto its base. The gap would fuse | separate pieces, with a note |
| anything else | a lid resting on its base, a wheel beside a car | separate pieces |

The thinness test is what tells a stripe from a figure standing on its
plinth: both touch over a good share of the smaller body, but the stripe is
0.6 mm thick over its contact and the figure is its whole height. The gap
test is what tells a print-in-place wheel (0.35-0.4 mm clear) from a nut
threaded on its screw (0.1 mm): both are captive, only one prints that way.

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
the outward normals of its eight largest flat regions -- the list
`orient_for_print` uses, so each has a name a person recognises ("the
1200 mm² flat face") -- and the six largest facets of its convex hull,
which is what a part with no flat face of its own rests on. Each is scored by a **cost in mm² of support
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
triangles), then the full rules on the best four, on "as modelled"
(always reported as the baseline), and on upside down and the two largest
flat faces, which raw overhang can rank wrongly because it does not see
islands (a screw lying on its threads has little overhang and two dozen
islands).

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
`plate=` a plate number of its variant, `at=(x, y)` where the middle of its
footprint stands on the plate, `group=` (pieces that share a plate when
they fit, side by side), `coupon=True`. Pinned choices are reported as
pinned, and a pin that cannot be kept is a note, never a silent change.
Two pieces each pinned their own way up are never joined into one
print-in-place piece: joining would drop one of the pins.

**Rejected:** a continuous search over rotations (it finds a 23° tilt
nobody can explain or reproduce, and tilts print badly on flat-faced
parts); the normalised weighted score of `orient_for_print` (a score
relative to the other candidates cannot be compared between parts or
explained in mm²); scoring with the whole `print_check` (wall thickness
and manifoldness do not change with orientation and are the expensive
half).

**Judging a hand-made layout.** The same scoring answers "is this part
placed well?": `assert_orientation_optimal` (and `cadpole check
--orientation`) compares a part as it stands with the best candidate and
fails, or warns, past a tolerance in the same mm² units, naming the better
orientation and the saving. `cadpole plate` reports the script's own
hand-placed print layout the same way, beside its plan. The golden tests
use it to hold the plan to the examples' own layouts: never more support,
never more plates.

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

The layout engine (`cadpole_print::plate::pack`, checked by
`plate::verify`) answers three questions in order: which plate each piece
goes on, where on the plate it stands, and whether the result keeps its
promises.

### Footprints

A piece's footprint is its triangles projected onto the bed and
rasterised at 1 mm: a cell is marked when the piece touches any part of
it, so the piece lies inside its cells. Each footprint is kept twice: the
cells themselves (the *core*), and the core grown by the `spacing` (4 mm
by default), measured between the cells' squares. A plate is a bitmap of
where every nozzle reaches (the H2D's and H2C's X 25-325, the whole bed
for a one-nozzle machine) holding the *grown* rasters of the pieces on it,
the excluded corners (the X1 and P1 cutter) and the prime tower's square;
a piece may stand wherever its *core* misses them all. That makes the
spacing a guarantee and not an average: a core that misses a grown raster
is a shape at least `spacing` from the other. Bitmap rows are 64-bit
words, so a test is a few word ANDs per row.

Because the test is on outlines, an L-shaped base and a flat lid nest
into each other's bounding boxes, small parts stand in the two triangles
beside a long part laid across the plate, and a ring takes another part
inside it.

**Spins.** A piece the script has not pinned is tried at 0°, 90°, 180°,
270° and the four quarter turns of its minimum-area rectangle; a piece
longer than 0.8 of the plate's shorter side is also tried with its long
side along either diagonal of the plate, both ways round. A later spin
must be better by more than a cell to be chosen: a round part is not
turned for nothing. `spin=` pins the turn and `spin=False` forbids one.

### Which plate

A plate costs time to clear and start again; a colour change costs the
swap and the purge, on every layer where a plate mixes filaments, and a
plate that mixes them at all needs a prime tower that takes a corner of
the bed and grows as tall as the highest mixed layer. A plate of one
filament has no tower, no purge, and the whole bed. So the densest packing
is not always the cheapest print, and the choice of plates is made on
both:

```
cost = plates × 1800 s  +  colour changes × (40 s purged, 10 s by a change of hotend)
```

with the colour changes estimated as below (*Colour*). The numbers are
Cadpole's own round figures: half an hour for someone to come back,
clear the bed and start the next plate; forty seconds for a change that
purges the nozzle; ten for one that only changes hotend. A change is a
change of hotend when the plate has no more filaments than the machine
has hotends -- 2 on an H2D, 7 on an H2C, 1 on a one-nozzle machine --
and then it purges nothing either. So the same model plates differently
on different machines: a white screw and a black nut share a plate on an
H2C and take one each on an X1. `print_plan(..., prefer=)` chooses what is
minimised: `"balanced"` (the default) the sum above; `"plates"` the fewest
plates, then the fewest changes; `"waste"` the fewest changes, then the
fewest plates.

The candidates come from one fill run several ways. A fill takes the
pieces in order and gives each to the first plate that takes it; plates
are tried fewest new filaments first, then most filaments shared, then
fewest filaments in all (so a one-filament piece joins a one-filament
plate before a mixed one), then plate number; within a plate the piece
goes to the lowest, then leftmost, clear position over its spins and its
equally good orientations. The fill is run in three orders (largest
footprint first, longest first, tallest first, a `group=` moving as a
unit) and with two rules for joining a plate: *wherever it fits*, and
*only where it brings no new filament*, which keeps filaments on plates of
their own. Each fill is then improved by *emptying plates*: the smallest
plate's pieces are moved onto the others wherever that is cheaper, until
no plate can go -- a short piece in another filament costs a few changes
on a plate of tall ones, and more than that on a plate of its own. Up to
six fills are costed and the cheapest kept, the first found among equals.
When keeping filaments apart won over a packing with fewer plates, a note
says how many colour changes it saves.

A plate never takes a piece that would put it past `ams` filaments. A
piece pinned with `plate=` goes on that plate or, when it cannot stand
there, is placed elsewhere with a note; when the pins cost colour changes
that leaving the pieces free would save, a note says how many. Pieces of
one `group=` are kept on one plate when they fit, and a note says when
they do not. A piece that fits no plate in any spin is put alone on a
plate nothing else may join, with a note.

Each set of plates reports the least number of plates its pieces' area
and filaments allow, so "2 plates" can be read against "1 at least".

### Where on the plate

With the plates settled, each is laid out again for quality, from the
middle of the bed outwards. The middle is where a bed's heat is most
even, so it goes to the pieces likeliest to lift: each piece has a
*warp need* from 0 to 1,

```
need = 0.4 length/250 mm + 0.3 first layer/15000 mm² + 0.2 flatness + 0.1 sharp corners
```

(each term clamped to 0..1; flatness is how low the piece is for its
length, and it and the corner term count only as far as the piece is
long or broad). A long two-layer panel scores near 1, a small chunky part
near 0. Pieces are placed in order of `area × (0.25 + need)`, each at the
clear position *nearest its target*, found by scanning positions outwards
from the target. The target is the middle of the plate, drawn towards the
pieces of the same filament already down (weight 0.25) and of the same
`group=` (1.5), against the middle's own weight `centre_bias × (0.3 +
need)`. Nearest-first placement keeps the pieces in one compact cluster,
which is also the short travel, and puts same-filament pieces and groups
side by side. The prime tower keeps a corner (back right first), never
the middle.

When a piece does not fit that way the plate is tried again largest
first; failing that, the fill's own layout is kept and moved as a group
to the middle of the plate. The plate count is never traded for any of
this. `centre_bias` is a property of the printer profile (1 by default);
`print_plan(..., centre=)` overrides it, and `centre=False` packs from the
front left corner and centres the group, which is what a bed with even
heat wants.

`print_part(..., at=(x, y))` pins the middle of a piece's footprint to a
point on the plate, in mm from the bed's front left corner. Pinned pieces
are placed first and everything else is packed round them.

### The guarantees, and how they are checked

The raster is where the search happens; it is not what is trusted. After
packing, `plate::verify` projects every piece's triangles as they finally
stand and measures, exactly:

| invariant | meaning |
| --- | --- |
| `gap` | no two pieces on a plate are closer than `spacing` (the least distance between their projected triangles) |
| `reach` | every piece is inside what every nozzle reaches |
| `keep-out` | no piece stands in an excluded corner or in the prime tower's square |
| `filaments` | no plate of several pieces needs more filaments than `ams` |
| `once` | every piece is placed exactly once per set of plates |
| `pinned` | a pinned plate, spin or position is honoured, or a note names the piece |

A breach is answered by packing again with a wider margin, and is a
warning naming the plate, the rule and the pieces if it survives two more
tries. None is expected: the raster test is conservative by construction.
The report says the check ran, and each plate's line gives the closest
pair's measured distance.

The project writer leaves a planned plate exactly as it stands: it does
not centre the pieces again, takes the planner's corner for the prime
tower, and does not compare bounding boxes, which overlap whenever pieces
nest. Copies made by `print_plan` carry their plate, so a script that
exports them itself with `export(..., flavor=flavor.bambu, plate=...)`
keeps the layout, as long as one plate's pieces stay together on one
plate; pieces from two plans mixed on a plate are the script's own
arrangement again.

Nothing is random and nothing depends on hash order: the same input
gives the same plates. A dozen pieces plan in about 0.1 s and thirty
large ones over five plates in under a second
(`//src/rust/cadpole-opt:plate_assignment_test` prints the times).

### What was tried and rejected

* **Rectangles** (shelf or guillotine packing): a long part across the
  plate wastes both triangles beside it, and concave parts cannot nest.
* **No-fit polygons**: exact, but fragile on real meshes (thousands of
  near-collinear silhouette edges, holes, slivers) and it needs a robust
  polygon offset and Minkowski sum to be written or brought in. The
  raster gives the same nesting at 1 mm with a guarantee that is checked
  on the triangles afterwards.
* **Randomised search** (annealing, genetic): the same script must give
  the same file.
* **A mixed-integer program for the choice of plates.** Assigning pieces
  to plates to minimise plates and colour changes under area and filament
  limits is a natural MILP, and `opt` can already solve one. It is built
  and compared in `//src/rust/cadpole-opt:plate_assignment_test`, which
  pins the program's assignment as `plate=` and plans it to learn whether
  it fits and what it costs. The program knows the pieces only by area,
  so on the fuller cases its first answer does not nest and it has to be
  solved again with less area; the solver is native only, so a plan made
  in the browser would differ from one made by the CLI; and it is slower
  on the largest case (6 s against 0.8 s for thirty pieces). It did find
  one thing the fills missed -- two plates and ten colour changes where
  they kept three plates and none -- which is what the emptying pass is
  for; with it the planner's cost equals the program's on every case. The
  test keeps that honest: it fails if the planner falls more than a tenth
  behind.
* **A linear program to squeeze the layout** after placement: it needs
  the pieces' separation written as linear constraints, which is the
  no-fit polygon again. Nearest-first placement already leaves the
  cluster compact.

## Colour

A piece's filaments are its bodies' colours and their paint, in the
order the project assigns slots (first use). The report counts, per plate,
the filaments, how tall its prime tower grows (the highest layer that
mixes filaments), the filament purged (300 mm³ a change, none when every
filament has a hotend of its own) and an **estimate of colour changes**:
the plate is cut into
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
checked on the pieces' real outlines: none closer than 4 mm to another, all inside the plate, none in a prime tower's square or an excluded corner.
plate 1 "Parts": 4 pieces, 2 filaments, ~410 colour changes
  layout: the pieces take 122 x 105 mm, centred on the bed; the closest two are 4 mm apart (at least 4 asked); prime tower in the back right corner, 11.6 mm tall, ~123 cm³ purged
  base: as modelled; 0 mm² support (best of 7)
  lid: upside down (on the 2140 mm² flat face): 0 mm² support vs 340 mm² as modelled
  clip coupon: turned so the clip arm runs along the layers (along= (1, 0, 0))
  gear set (sun, 3 planets, ring, carrier): print-in-place (captive within 0.35 mm), kept as modelled
```

It is returned as text and as data (`plan["pieces"]`, `plan["plates"]`),
printed by `cadpole plate`, and returned by the `Plate` operation.

## Settings without anybody's project

Plating never needs a saved slicer project. The written project's printer,
plate, filament and process scaffolding is generated from Cadpole's own
printer table (`cadpole_print::printers`): the bed, the nozzle reach, the
excluded corners, the stock preset names, the filament colours. A
`template=` is an optional *settings source* and nothing more: the
writer reads its `project_settings.config` and the version that saved it,
and no object, plate picture, cut information or per-plate file of it ever
reaches the output (`a_template_brings_its_settings_and_none_of_its_objects_or_pictures`).

**Purge volumes.** A project of several filaments carries a purge
(flush) volume for every change of filament, `flush_volumes_matrix` with
`flush_multiplier` 1; a slicer reads a project without one as "purge
nothing" and the colours mix. With no template the volumes are Cadpole's
own estimate from the two colours (`cadpole_export_3mf::purge`):
`100 + 250·d + 450·max(0, ΔL*)/100` mm³, clamped to 100..800, where `d` is
the sRGB distance between the colours (0..1) and `ΔL*` how much lighter the
new filament is. Black to white purges 800 mm³, white to black 350, two
near-identical colours 100. A template's own table is kept when it has a
volume for every change; otherwise the estimate replaces it, with a warning.

## Follow-ups

* A per-piece "held but tight" choice: a lid snapped onto its base is
  printed apart today; a script that wants it printed closed says so with
  a list.
* Faster evaluation of very large field bodies before planning (the
  planner itself takes well under a second; a fine organic body's
  evaluation dominates).
* Automatic detection of snap arms (a cantilever whose root is much
  thinner than its body) to set `along=` without the script saying so.
* A finer raster near tight fits: at 1 mm two pieces end up 4 to 5.5 mm
  apart for a 4 mm spacing.
* Keeping tall pieces from standing between the prime tower and the
  pieces it serves, and ordering pieces on a plate by height.
* Which filament is on which nozzle: the colour cost counts hotends, not
  which nozzle a filament is loaded on.
* `cadpole plate` flags for `prefer` and `centre` (they are `print_plan`
  arguments today).
* A real purge volume per pair of colours in the plan's estimate (the
  written project already carries one per pair).
* Per-plate layer-height variants, and orienting for the visible surface
  (seam and top-surface placement).
* Splitting a piece that fits no plate, with `split()` and `pegs()`.
* The web UI's *Plates* toggle showing the plan's report beside each plate.
