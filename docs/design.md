# Cadpole design

How Cadpole works and why. `docs/architecture.md` is the map of the
repository and the transports; `docs/starlark-api.md` is the script API, with
`docs/parts.md` and `docs/buildings.md` for the two kinds of work. This
document describes the mechanisms underneath.

Contents:

- [Goals and non-goals](#goals-and-non-goals)
- [Script-first](#script-first)
- [Document, model, session](#document-model-session)
- [Feature tree](#feature-tree)
- [Topological naming](#topological-naming)
- [Kernels](#kernels)
- [3D-printing checks](#3d-printing-checks)
- [Rendering](#rendering)
- [Session and event protocol](#session-and-event-protocol)
- [Parameter sweeps](#parameter-sweeps)
- [Assemblies](#assemblies)
- [Operations, CLI and crates](#operations-cli-and-crates)
- [Heavy work in the browser](#heavy-work-in-the-browser)
- [The limited-privilege listener](#the-limited-privilege-listener)
- [Testing](#testing)
- [The 2D constraint solver](#the-2d-constraint-solver)

## Goals and non-goals

**Goals**

- Script-first parametric solid modeling for making real things, mostly for
  3D printing (planters, brackets, enclosures, organic and sculpted shells).
- Two kinds of work on that one model: parts and assemblies
  (`docs/parts.md`) and buildings and plans (`docs/buildings.md`). A
  building is ordinary bodies; what the building side adds (feet and inches,
  terrain, sun and shadow, sightlines, floor plans, the `opt` solver) is
  libraries and script globals over the same feature tree, kernel, tests,
  drawings and exports, never a second engine.
- One engine everywhere: Linux and macOS native, WebAssembly in the browser,
  and a C ABI for other front ends.
- Assistants are first-class users: the API is documented as a
  machine-readable definition, the UI exposes what the human is pointing at,
  and rendering and sweeps let an assistant see alternatives.
- Everything persisted is protobuf; everything built is Bazel.
- Apache-2.0, and only permissively licensed dependencies.
- 2D drawings as views of the model: plans, sections and hidden-line views
  with dimensions are derived, never drawn (`docs/drawings.md`).

**Non-goals**

- A direct-manipulation modeler. The UI selects and inspects; it does not
  create features. Feature creation is the script's job.
- Interactive 2D drafting (hand-placed lines and dimensions that are not
  derived from the model), CAM, simulation.
- Multi-user editing. One session, one document, several *clients* (UI, CLI,
  assistant) is the concurrency model.

## Script-first

The Starlark program *is* the model. Evaluating it produces a feature tree,
bodies, meshes and diagnostics; none of that is stored as truth.

A text model is diffable, reviewable, assistant-editable and trivially
parametric. Starlark specifically because it is deterministic, hermetic (no
I/O), has a Rust implementation (`starlark-rust`, Apache-2.0) that compiles to
wasm, and reads like Python.

## Document, model, session

| Thing | Written by | Persisted? | Contents |
| --- | --- | --- | --- |
| `Document` (`document.proto`) | user, assistant | yes, protobuf | script files, param overrides, selection, view state |
| `Model` (`eval.proto`) | engine | no | params with effective values, features, bodies (topology, mesh, material), diagnostics |
| `Session` | `cadpole serve` | no | the document, its revision, connected clients, messages, the event log |

Each has a different lifetime and a different writer: a document is edited by
the assistant and the user, a model is recomputed whenever the document
changes, a session exists only while the server runs.

Evaluation is a pure function `Document × (tolerance, options) → Model`. The
web UI runs it in wasm; the CLI and server run it natively. It is the same
crate in both.

## Feature tree

Features are created only by script calls. Each call records:

- `id`: the `name=` argument if given (unique, non-empty, and without `:`,
  `#` or `/`), else `<kind>_<n>` numbered per kind in call order.
- `kind`, `inputs` (feature ids consumed), `outputs` (bodies produced),
  `args` (stringified, for the inspector), `span` (source location), and
  per-feature diagnostics.

A body id is `<feature id>#<n>`, where `n` is the body's position in the
document's creation order (one counter for the whole evaluation, starting at
0). Inserting a feature that makes a body earlier in the script therefore
renumbers the bodies after it; the feature id is the stable half.

A body that is consumed by a boolean or a transform is hidden, not deleted,
so the tree remains a complete history.

Feature ids are numbered by call order rather than hashed from the call site
so that renaming a variable or adding a comment does not rename features.
Adding a call before another one of the same kind does renumber, which is why
anything referred to again should be named (`name="lid"`).

## Topological naming

Faces, edges and vertices need names that survive re-evaluation, because a
selection made in the UI must still point at "the top face of the lid" after
a fillet radius changes three lines earlier.

### The naming grammar

This section is normative. `cadpole_core::naming` implements it and is the
only place in the code that does.

Every feature that creates geometry names what it creates from *how* it was
created, never from an index into the kernel's output order.

| Feature | Face names | Notes |
| --- | --- | --- |
| `extrude` | `start`, `end`, `side[l.s]` | `l` = loop index in the sketch, `s` = segment index in that loop. `start` is the cap on the sketch plane. |
| `revolve` | `seg[l.s]`, plus `start`, `end` for partial revolves | one revolved surface per profile segment |
| `loft` | `start`, `end`, `side[l.s]`, `side[l.s]#b` | A ruled loft through more than two sections has one side face per *band*: `side[l.s]#b` is the band between the section the script listed `b`-th and the next, and the unsuffixed `side[l.s]` is the group alias for the whole column. The band index is the script's list index. Under `loft(match="arclen")` `s` indexes the matched segment list (the union of both sections' vertices). A `loft(ruled=False)` skin has no bands, so its side faces are the plain `side[l.s]`. A ruled cap is several faces, `start#k` / `end#k` in order from the loop's seam, under the group alias `start` / `end`. |
| `sweep_along` | `start`, `end`, `side[l.s]`, `side[l.s]#b` | `b` is the path segment; the plain `side[l.s]` when the path has one segment |
| `split` | `cut` on both pieces; every other face keeps the name it had | the pieces are ordered along the cutting plane's normal, negative side first |
| `box`, `cylinder`, `cone` | `top`, `bottom`, `front`, `back`, `left`, `right`, `side` | the extrude names stay on as aliases, so `box_1/end` and `box_1/top` are the same face. A circle is two arcs in the sketch, so `cylinder_1/side` is a *group* of two faces, `side#0` and `side#1` |
| `sphere`, `torus` | `surface`; `outer`, `inner` | the revolved primitives. A sphere is two caps, `surface#0` and `surface#1` under the group `surface`; a torus's tube circle is an outer and an inner arc, each in the pieces the kernel cuts a full turn into, `outer#k` and `inner#k`. `revolve`'s `seg[0.s]` stays on as the alias |
| `chamfer` | `face[<edge>]` | the chamfer face, named after the edge it replaced |
| `fillet` | `blend[<edge>]`, `vertex[<vertex>]` | a blend per edge; a sphere octant where three blends meet at a corner |

```
ref      := feature "/" local
         |  feature                       # the feature's primary body
feature  := ident                          # `name=` or <kind>_<n>
local    := "start" | "end" | "surface"
         |  "top" | "bottom" | "front" | "back" | "left" | "right" | "side"
         |  "outer" | "inner"              # primitives; the generic form
                                           #   stays on as an alias
         |  "side[" l "." s "]"            # extrude, loft, sweep: loop.segment
         |  "seg[" l "." s "]"             # revolve
         |  "cut"                          # the face split() exposed
         |  "face[" edge "]"               # a chamfer's face
         |  "blend[" edge "]"              # a fillet's blend
         |  "vertex[" vertex "]"           # a fillet's corner patch
         |  "face[" n "]"                  # unmatched fallback, with a warning
         |  import_path                    # see below
         |  local "#" k                    # k-th piece, canonical order
edge     := ref "|" ref                    # canonically ordered face pair
         |  edge "@" k                     # ordinal when a pair bounds several
vertex   := ref "|" ref "|" ref
         |  vertex "@" k
import_path := ident ("/" ident)*          # file structure of a 2D import,
                                           #   e.g. site/PL/loop[0]
```

Full names are `<feature id>/<local name>`, e.g. `extrude_1/side[0.2]`,
`lid/end`, `lid/end|side[0.2]`. In an edge or vertex name the feature prefix
is written **once** when every part shares it (`lid/end|side[0.2]`) and in
full when they do not (`block/end|hole/side[0.0]`). The parts are sorted, so
the pair has one spelling. A seam that a single face meets itself along keeps
the pair shape (`lid/side|side`). `@k` attaches to the whole edge or vertex
name, not to a `local`.

Rules:

- **Multi-output features use `#n` on the *body* id, never brackets on the
  feature id.** There is no `ext[0]` form.
- `face:N` / `edge:N` / `vertex:N` (kernel output order) is **not part of the
  grammar**. Resolution accepts it as an *addressing form*, and
  `TopoFace.index` / `TopoEdge.index` publish it beside the real name. It is
  unstable across sketch edits: change a parameter and `face:3` may be a
  different face. A ref carrying one resolves, and
  `ResolvedRef.resolved_name` says which real name it landed on.
- **Splits are ordered geometrically, never by kernel output order.** When a
  name ends up on several faces of one body (a boolean cut one face in two,
  or a body was united with a copy of itself) the pieces become `name#k`,
  ordered by `naming::order_pieces`: the world-space centroid's `x`, `y`, `z`,
  consulted in order of their spread across the group, largest first; then
  the larger area. Two key values closer than a tie floor (ten microns, or a
  thousandth of the group's extent when that is more) count as equal and fall
  through to the next key. The unsuffixed name stays on as a group alias and
  resolves to the *set*.

Because sketch loops are ordered by call order and segments by construction
order, these names are stable under parameter changes and under edits to
unrelated features.

### Names through every feature

Kernels do not propagate identity, so Cadpole re-derives names at every
feature. `cadpole_core::naming` touches no kernel: it is handed `FaceProbe`s
(surface kind, centroid, normal, area and a few points *on* the face) and
either a `Generator` (what the feature did) or a `SurfaceCatalogue` (the
operands' named surfaces as triangles in a uniform grid).

1. **Generators.** Naming is geometric, not positional: a cap is a planar
   face every one of whose sample points sits at 0 or at `distance` along the
   sweep, and a side is attributed to the `(loop, segment)` whose curve its
   points are nearest in sketch coordinates. The nearest planted point comes
   from an exact k-d tree that breaks ties in list order, so naming `F`
   faces costs `F log F`: a loft through sixty sections of sixty points
   names its 3,600 side faces in a few milliseconds.
2. **Transforms** (`translate`, `rotate`, `copy`, patterns, `mirror`,
   `scale`): faces map 1:1 and names are kept, so `lid/end` stays `lid/end`
   after a translate and `blk/top` is still `blk/top` in a mirror image.
3. **Booleans** on the B-rep path match each result face's sample points to
   the nearest source triangle in the catalogue. Each face gets a role:
   - `kept`: the source that owns most of the samples (at least a fifth of
     them) gives the name.
   - `merged`: any other source owning at least a fifth of the samples is
     added as an alias, so a face fused from two coplanar faces answers to
     either name.
   - split: several result faces landing on one source become the `#k`
     pieces.
   - `unmatched`: a face that matches nothing is `<feature>/face[n]` with a
     warning diagnostic.
4. **Booleans on the mesh path** do not match geometrically: manifold's
   per-triangle provenance ids carry the names through, and a split shows up
   as several connected components under one id.
5. **Edges** are a face pair plus, when that pair bounds several edges (a
   cylinder cut by a plane gives two), an ordinal: `body/side[0.0]|end@1`.
   Vertices are the faces that meet there.

Both kernel paths finish in the same `Topology::apply_names`, which is why
the two kernels produce the same names for the same model.

**Surface classification.** truck's surface types are only plane, B-spline,
NURBS and revolved curve, so `cadpole_core::surface_class` recognises
quadrics itself: every face is sampled, fitted and verified to a micron at
tessellation, `FaceInfo.surface` carries the class with its parameters, and
`Face.kind` reports it.

| truck construction | recognised as |
| --- | --- |
| `Plane` | `Plane{origin, normal}` |
| NURBS from `tsweep` of a circular arc | `Cylinder{axis_point, axis, radius}` |
| `RevolutedCurve` of a line | `Cone{apex, axis, half_angle}` / `Cylinder` |
| `RevolutedCurve` of an arc | `Torus{center, axis, major, minor}` / `Sphere{center, radius}` |
| `BSpline` from `homotopy` of two lines | `Plane` (re-fit within tolerance) |
| `BSpline` from `homotopy` of two arcs | `Cone` |
| anything else | `Other`: a free-form surface, or one analytic only to a looser tolerance than a micron |

Surface identity is therefore a comparison of a handful of parameters, never
of NURBS control nets. `TopoFace.surface_kind` and `TopoEdge.curve_kind` in
`eval.proto` publish the class, and `faces(kind=)` / `edges(kind=)` reject an
unknown string with the valid list.

### Selection refs and staleness

An `EntityRef` (`document.proto`) stores the feature id, the body id, the
kind, the topological name and an optional `EntityFingerprint`. It is resolved
against the model on every evaluation by `naming::resolve`:

| Step | Outcome |
| --- | --- |
| the name (or an alias) matches exactly one entity | `ok` |
| the name matches several (a face that was split, or a group name such as `cylinder_1/side`) | `ambiguous`: the ref resolves to the *set*, flagged; never a silent pick |
| the name is in the `face:N` index form and in range | `ok`, with `resolved_name` set |
| no name match, but exactly one entity matches the fingerprint | `moved` |
| otherwise | `stale` |

A short local name (`"end"`) is also tried qualified by the ref's feature.

Stale and ambiguous refs are *kept* in the selection, flagged, and shown
greyed in the UI with the reason, so the user or the assistant can re-pick.
They are never silently dropped: dropping loses intent. `cadpole selection`
prints the status per ref.

`ResolvedRef` (`ops.proto`) carries `status`, a one-sentence `reason`,
`resolved_name` (which real name the reference landed on, so an alias, an
index form or a `moved` match is never invisible) and `matches` (the whole
set, when the status is `ambiguous`). An ambiguous reference comes back with
no geometry, so a client cannot mistake a set for a pick.

**Selectable kinds** (`EntityKind` in `document.proto`): feature, body, face,
edge, vertex, sketch, sketch entity, param, asset.

### Geometric fallback

`EntityFingerprint { surface_kind, centroid, normal, area, length }` is
captured when a selection is made. When the name no longer resolves, a
candidate matches the fingerprint when the kind agrees, the centroid is
within `max(1 mm, 2 % of the body diagonal)`, the normal within 5° and the
area (or length) within 25 %. Exactly one match is `moved`; zero or several
is `stale`. This deliberately under-matches: a wrong silent match is worse
than an honest "stale".

The fingerprint is optional. A reference without one whose name is gone is
simply `stale`, and the reason says so. The body diagonal is measured over the
face centroids of the body the reference belongs to.
`cadpole_core::naming::Fingerprint` is the comparison.

### Selectors in scripts

`cadpole_core::select` holds the filters, the order and the reducers, and
touches no kernel: it is handed `Candidate`s built from the tessellated
topology (name, aliases, surface or curve class, centroid, direction, area,
length, arc radius, convexity, dihedral, exact bounds and points *on* the
entity) and returns indices plus, when it returns none, one sentence saying
which filter rejected everything and by how much.

`faces()` and `edges()` take `on_plane=`, `above=`, `below=`,
`inside=(lo, hi)`, `nearest=`, `kind=`, `name=` (a `*`/`?` glob over the
generated name, an alias, or the local part), `within=` (angular tolerance in
degrees), `tolerance=` (lengths in mm) and `allow_empty=`. `faces()` adds
`normal=` (signed) and `min_area=`/`max_area=`; `edges()` adds `direction=`
(unsigned, and only straight edges match), `of_face=`,
`min_length=`/`max_length=`, `min_radius=`/`max_radius=`, `convex=` and
`min_dihedral=`/`max_dihedral=`. A plane or half-space test takes the *whole*
entity, never its centroid, and is exact for a planar face. An edge's
`"circle"`/`"arc"` kind and its radius are measured by a circle fit with a
residual check.

A `Selection` supports `count()`, `names()`, `one()`, `nth(k)`, `largest()`,
`smallest()`, `nearest(point)`, `max(axis)`, `min(axis)`, and the set
operations `|`, `&`, `-`. The full signatures are in `docs/starlark-api.md`.

**Selection order is geometric and is defined once**, in `select::sort_key`:
the centroid as `(z, y, x)`, then the lower bounding-box corner the same way,
then the larger entity, then the generated name: lowest first, then front to
back, then left to right. Coordinates are quantised to a micron by
`naming::quantize` before they are compared, so entities that are nominally
at the same place cannot swap on float noise. A `Selection` stores each
member's rank in its body's full canonical order, so `a | b` comes back in the
order one selector asking for both would have produced. Nothing in the layer
reads a `Vec` index, a hash iteration order or a tessellation order as data.

**An empty selection is a warning, not an error; consuming one is an error.**
`faces(kind="revolved")` on a box legitimately has none and `count() == 0` is
assertable, so the set may be empty. It warns with the reason and the nearest
miss (`allow_empty=True` silences it). But `fillet(part, edges=<empty>)`,
`sketch(on=<empty>)` and `.one()` on anything but one entity are errors: an
operation may not act on nothing.

**`sketch(on=<face>)`.** A planar face carries a sketch whose normal is the
face's own (so a positive `extrude` leaves the body and a negative one cuts
in), whose origin is the face's area-weighted centroid, and whose x direction
is `naming::canonical_frame` of the normal. That frame is derived from the
orientation alone, so it cannot spin when the outline changes, when the face
slides within its plane or when the normal flips, and it reproduces
`XY`/`XZ`/`YZ` exactly for the axis-aligned cases. A non-planar face is
refused with a diagnostic naming the surface kind. The face's name is recorded
in the sketch feature's args.

### `ExplainRef`: selection traces back to code

Selecting a face must show the feature that created it and the source span.
One operation answers it (`ExplainRefResponse` in `ops.proto`, implemented in
`cadpole-engine/src/explain.rs`), and `cadpole selection`, the MCP tool and
the inspector render the same message:

- `ref`, `status`, `reason`: the reference after defaults were filled in, and
  how it resolved.
- `chain`: the lineage, newest first, as `Provenance` entries (feature id,
  kind, role, span, inputs, args). It is the *principal* path: a feature's
  first input is its base, so following it reaches the sketch.
- `contributors`: every other feature reachable through `inputs`, which is
  where a boolean's tool bodies end up.
- `generator`: the sketch the chain bottoms out at, with its span.
  `Generator.loop` and `Generator.seg` are always 0: per-segment provenance is
  not implemented.
- `summary`: one paragraph assembled from the structured fields.
- `face` / `edge`: the geometry, when the reference named one that resolved.

`role` is derived from the consuming feature's kind: `new` for the head,
`merged` under a boolean, `copied` under a pattern, `kept` otherwise. The walk
is over `Feature.inputs`, is depth-bounded at 256 and detects cycles. With no
reference given it explains the session's first selected entity.

## Kernels

Two kernels, one `Body`:

```rust
enum Body { Brep(..), Mesh(..) }   // body.kind == "brep" | "mesh"
```

A `Field` (a signed distance field) is a third representation, but it is a
*value*, not a body; see [Fields](#fields).

### B-rep: truck

`truck` (Apache-2.0, pure Rust, compiles to wasm) provides planar faces,
extrusion (`tsweep`), revolution (`rsweep`), booleans (`shapeops`) and exact
surfaces. Exact faces are what make "select the top face" and STEP export
possible. Cadpole wraps it in `cadpole_core::kernel` and works around its
limits as follows.

| truck limit | What Cadpole does |
| --- | --- |
| no 3D fillet or chamfer | Sketch-level 2D fillets and chamfers; an exact chamfer by cutter (`cadpole_core::chamfer`); and constructed blend faces (`cadpole_core::fillet`): cylinders along straight plane–plane edges, tori round circular edges, elliptical mitres where two blends turn a corner, and a sphere octant where three meet at a box corner. Anything the planner refuses is a diagnostic; `kernel="field"` is the other path, by request only, and gives a mesh body. |
| `shapeops` needs transversal intersections: coincident faces or touching-only operands fail, and operands whose boundaries never meet are classified wrongly | Every pair is classified before the call; see [Contact handling](#contact-handling). |
| `shapeops` returns its faces in an order that depends on pointer addresses | `kernel::canonical` sorts a boolean result's shells and faces on a geometry-only key at the kernel boundary, so two evaluations agree. |
| one `Solid` is one outer shell plus voids, and a boolean folds every shell into one lump | A boolean *on* a body that already has a cavity is refused on the B-rep path with a diagnostic, and `kernel="auto"` routes it to the mesh kernel. |
| slow booleans on curved pairs, with no cancellation hook | The route is chosen *before* the call from a face-count budget; see [Routing](#routing-between-the-kernels). |
| every non-planar face is NURBS, B-spline or a revolved curve | `cadpole_core::surface_class` recognises quadrics (see [Topological naming](#topological-naming)). |
| no loft or sweep along a path | `loft` is built on `try_wire_homotopy` and stitched into one solid; sections are matched in construction order, or with `match="arclen"` cut exactly at the union of each other's normalised arc lengths (lines stay lines, arcs stay arcs of the same circle). `loft(ruled=False)` runs one interpolating B-spline surface through every section. `sweep_along` sweeps a profile along a path. |
| its STEP writer emits boolean-result edges as intersection curves over polylines | Cadpole does not use that writer. `cadpole-export-step` writes the file; an intersection edge becomes a cubic B-spline refit when the refit verifies against the polyline within tolerance, and the exact `POLYLINE` when it does not. |
| tessellation is not welded by construction | Cadpole tessellates itself; see [Tessellation](#tessellation). |

### Contact handling

`kernel::disposition(a, b, tol)` runs before every B-rep boolean and returns
one of four answers. Only `Meeting` reaches `shapeops`.

| Disposition | subtract | intersect | union |
| --- | --- | --- | --- |
| `Meeting`: the boundaries come within `tol` | `shapeops` | `shapeops` | `shapeops` |
| `b` strictly inside `a` | `a`'s shells plus `b`'s shell inverted: one solid with a void | `b` | `a` |
| `a` strictly inside `b` | error: the result is empty and Cadpole has no empty body | `a` | `b` |
| wholly apart | `a` | error: empty, with the bounding-box gap in the message | error: two lumps are not one B-rep solid |

Both operands are tessellated at the boolean tolerance, bounding boxes reject
the pair, a BVH answers "does any triangle of `a` come within `tol` of one of
`b`", and one generalised winding number per operand decides containment when
nothing meets. The answer is accurate to the chordal sag of that
tessellation; a pair in doubt is reported `Meeting`.

A `Meeting` pair is classified a second time by `contact::plan`, which finds
every pair of faces that lie in the same plane and share area:

| Contact | Operation | What happens |
| --- | --- | --- |
| no coplanar pair | none | `shapeops` |
| additive, faces back to back, no shared volume | **glue** | the touching faces are dropped and the two shells are stitched into one, against a dictionary of vertices and edges keyed on geometry. The two shapes accepted are "the same boundary" and "one strictly inside the other", the second leaving a hole. No `shapeops` call and no surface recomputed. |
| subtract or intersect, a tool face coplanar with a body face | **cutter over-extension** | the *tool* is stretched along that face's own outward normal by 4 · tol, but only after (a) the tool is shown to be a prism along that axis, face by face, and (b) the slab it gains is proved to miss the body. Additive geometry is never moved. A "coincident" pair with material between its planes (a floor or wall thinner than the tolerance) is refused, naming the thickness. |
| subtract or intersect, the tool only rests against the body | none | nothing is removed: `subtract` is `a`, `intersect` is an error |
| anything else | **diagnostic** | the two faces with their kinds and boxes, the plane and which way each looks, the shared area, the overlap on each axis, and the fix as a number |

`union` routes to glue whenever the pre-pass says the pair touches. `glue()`
also exists as a Starlark call whose value is the *refusal*: it is an error if
the bodies overlap, meet only along an edge, or coincide in part, where
`union` would run a kernel boolean instead.

Four contacts are refused on the B-rep path:

- a *flush* additive pair (both bodies on the same side of a shared plane),
  which needs the two faces merged in the plane;
- a *partial coincidence* (the body both ends on the tool's plane and crosses
  it);
- a glue whose faces overlap only in part;
- a *tangent* pair, where the two boundaries graze rather than cross. This
  one is found after the kernel fails.

Under `kernel="auto"` these fall through to the mesh kernel; under
`kernel="brep"` they are diagnostics that name the two faces, the measured
distance and angle, and the fix as a number.

**The tolerance ladder.** truck-shapeops also fails plain transversal cuts
(a 2 mm hole through a 3 mm plate, a coaxial cup at some radii) in narrow,
tolerance-dependent bands, more often at 0.1 mm than at 0.05. So an exact
boolean that fails, or returns a solid that does not mesh, is tried again at
0.4x and 0.2x of the boolean tolerance before it is refused, with an info
note on the feature when a finer rung was needed (`kernel::shapeops`). The
finer rungs are slow: truck at 0.04 mm costs a good deal more than at 0.1,
and a chain of cuts that each needed one ran far longer than the mesh
fallback it replaced (on a loaded machine, 16 holes of r = 3 cut one at a
time into a plate: about 1 s before the ladder, 40 s trying every rung on
every cut, 24 s starting each cut at the rung that last built; 32 small
holes: 0.4 s before, 15 s with the rung remembered). So one evaluation uses
the finer rungs once: after one pair has been built on a finer rung, or has
failed every rung, later pairs get a single attempt, as before the ladder,
and under `kernel="auto"` a failure goes to the mesh kernel. The 16 holes
then take about 0.6 s, and the 32 small ones under 1 s.
`examples/chess_rook.star`, whose cuts fail every rung, goes from 0.4 s to
about 2 s. Every successful boolean is also meshed at the boolean tolerance
and at the display tolerance to prove it meshes;
`examples/gridfinity_bin.star` and `examples/dice.star` measured within a
noisy 15 % of their times without that check. Pass `kernel="mesh"` where an
exact result is not needed.

**Shared planes on the mesh path.** Manifold resolves only exact
floating-point ties, so before every mesh boolean
`cadpole_core::tangent::snap_shared_planes` projects every vertex of the
second operand that lies within `max(0.1 µm, 1e-6 × the pair's diagonal)` of a
plane the two share onto the first operand's spelling of that plane. It
repairs two spellings of one plane and never moves a feature: the first
operand never moves, a wider gap is named rather than moved, and a snap whose
result is not a solid is declined in favour of the plain boolean. The B-rep
path never moves anything.

### Mesh: manifold-rust

`manifold-rust` (Apache-2.0, pinned at `=0.13.1`) is a pure-Rust port of
Manifold: topologically robust booleans with per-triangle provenance. It is
bound in one module, `cadpole_core::mesh_kernel`. Its `parallel` feature is
off: wasm32 is single-threaded and a sequential build is trivially
deterministic.

- Conversion in welds with `Mesh::welded` and refuses anything that then
  fails `is_watertight`.
- On the way out the result goes through `solid::validate`, and a body that
  is not closed and consistently oriented is a diagnostic rather than a body.
  `intersect` of two boxes sharing a face therefore says the operands touch
  without enclosing anything, instead of returning a zero-volume sheet.
- A union's result is also searched for crossings and doubled faces: rebuilt
  without the sliver cleanup when that is what caused them, refused
  otherwise.
- Provenance is manifold's per-triangle id, so a face in a result keeps the
  name of the face it came from rather than being re-derived geometrically.
- A model built out of boxes is joined by `union(weld=)` on a grid instead
  (`cadpole_core::gridjoin`): exact, with contacts along an edge or at a
  corner bridged, and closed and oriented by construction.
  `gridjoin::orthogonal_core` takes an orthogonal solid apart into the boxes
  it fills, which is how `cadpole check` fuses a piece printed as one body per
  filament.

**Vertex normals are the source surface's.** A face's surface travels with
its provenance (the recognised class, or for a free-form face the truck patch
itself), and the conversion back from manifold evaluates every result vertex's
normal on it, cheapest source first: a vertex the boolean left in place takes
its operand's normal (`CarriedNormals`); a new vertex on an intersection curve
takes the class's closed form, or is inverted onto the patch by a Newton
nearest-point search; and a group with no surface behind it (an imported or
field mesh) is area-weighted inside the group. A vertex on a group boundary
gets each group's own normal. Every fallback is counted on the trace
(`NormalStats`). `Mesh::smooth_seams`, which every rendered body passes
through, averages the vertices' *given* normals across a shallow seam instead
of re-deriving them from triangle planes, so the exact normal is what the
renderer shades with.

### Routing between the kernels

Booleans take `kernel="auto"|"brep"|"mesh"`. With `auto`, each pairwise step
is decided *before* the call:

1. either operand is already a mesh body → mesh kernel;
2. `faces(a) + faces(b)` over `kernel::BREP_FACE_BUDGET` (200) → mesh kernel;
3. otherwise truck, and on failure (coincident faces, an open shell, a panic)
   the same pair is retried on the mesh kernel.

Nothing else decides. In particular routing never depends on wall-clock time,
so the same document gets the same kernels on every run, on every machine and
in the browser. Every route that is not plain B-rep emits an info diagnostic
saying which rule fired. `kernel="brep"` disables the fallback: a failure is a
diagnostic, which is what a document that needs STEP wants.

| Operation | Result | Route |
| --- | --- | --- |
| `loft`, `cone`, `sweep_along` | Brep | not a boolean. The ruled band between two matched circular arcs is an exact cone. |
| Brep ∘ Brep | Brep, or Mesh when routed | the rules above |
| Mesh ∘ Mesh, Mesh ∘ Brep | Mesh | manifold; the B-rep side is tessellated at the document mesh tolerance and must pass the watertight gate first |
| Field ∘ Field | Field | graph composition; still a value |
| `mesh(field, resolution=)` | Mesh body | the only way a field becomes geometry |
| Mesh → Brep | not supported | STEP export needs B-rep; the diagnostic says so and suggests `kernel="brep"` on the boolean that left the B-rep path |

Conversions are explicit or come with a diagnostic. Print checks and mesh
exports always go through the tessellated mesh.

**A fan of tools is one boolean.** `subtract(body, tools)` and
`union(body, tools)` take a list, and folding that list pairwise makes every
tool pay for the whole accumulated body. `a − t₁ − … − tₙ` is
`a − (t₁ ∪ … ∪ tₙ)`, and when the tools cannot touch each other that union is
a concatenation with no boolean. `Context::boolean` therefore takes the
longest *run* of tools whose bounding boxes are clear of each other by more
than 0.01 mm and hands the whole run to `mesh_kernel::boolean_many` at once.
Three conditions:

| Condition | Why |
| --- | --- |
| the operation distributes over a union | `subtract` and `union` do; `intersect` keeps the fold |
| every step would route to the mesh kernel anyway | the B-rep kernel has no batch to offer. A chain that starts on truck, or that a script pinned with `kernel="brep"`, keeps the fold |
| the tools in the run cannot touch each other | decided on bounding boxes alone. Tools that cannot touch cannot cut each other, so no tool's face arrives split into `#k` pieces the one-at-a-time path would not have made, which keeps the names identical |

The run stops at the first tool that touches an earlier one; that one is done
pairwise and the run starts again after it, so operands are never reordered.
Provenance ids are handed out in operand order, so the result's faces keep the
order, names, aliases and per-face triangle counts the fold gave them. What
can differ is manifold's triangulation *inside* a face that several tools cut.

### Fields

A `Field` is a signed distance field: an expression graph of primitives,
booleans and smooth booleans, offsets, transforms and displacement. An
unmeshed field is not in the feature tree, not selectable, not exportable and
has no id. It becomes a body through exactly one call:

```
mesh(field, *, resolution, name=None, simplify=True, layer=None)   # global
field.mesh(resolution=..., ...)                                    # method form
```

`resolution` is the cell size in mm at the finest octree level. The result is
a mesh body like any other, watertight by construction.

`cadpole-field` implements it. The crate depends only on `cadpole-core`, has
no RNG and no global state, and builds for wasm32.

- **The tape.** An expression graph compiles to a flat instruction list with
  deduplication, plus a table of *spaces*: a transform is not a node, it is an
  entry in that table, and every node records which space it is evaluated in.
  Evaluating at a point walks the space table once and then runs the node
  loop; evaluating over a box does the same with conservative local boxes.
  There are two evaluators over one tape: plain `f64` where the surface is,
  and interval arithmetic to throw cells away. Every interval rule is an
  inclusion function, and a test samples each one to prove it.
- **Meshing.** Manifold dual contouring: an octree descended with interval
  culling; one QEF vertex per connected component of the surface inside a
  cell, clamped into its own cell; quads dual to every cut grid edge, split
  on whichever diagonal avoids a sliver. Watertight and 2-manifold are
  asserted on every result. Adaptivity is a bottom-up collapse under an error
  threshold, allowed only when one plane through the collapsed vertex
  reproduces every sign in the subtree.
- **Mesh → field.** A BVH over the triangles plus angle-weighted
  pseudo-normals for the sign, in a narrow band. So any body Cadpole can
  build can become a field.
- **Fillet and chamfer on real bodies** (`fillet(kernel="field")`,
  `chamfer(kernel="field")`). Not a smooth union of the whole body, which
  would round every edge, but a correction reconstructed per edge: the two
  adjacent faces' tangent planes are read off the mesh, the exact circular
  blend of that pair is computed, and the difference from the sharp corner is
  added to the body's own field, gated so it vanishes a fraction of a
  millimetre away. On a planar pair the result is the exact rolling-ball
  surface up to resampling, and the measured error is reported on every call.
  By default both operations are exact B-rep constructions instead.

### Height fields

`heightfield(...)` and `lithophane(...)` (`cadpole_core::heightfield`) make
a wall whose thickness follows a grid of values on a plane, a cylinder or a
revolved profile. The body is the image of the parameter box
`[0, U] × [0, V] × [0, 1]` under `B(u, v) + w · t(u, v) · N(u, v)`: base
point, wall thickness, unit normal on the relief side. Each of the box's six
faces is triangulated on one structured grid and wound by the box's outward
normal, and the whole mesh flipped when the map reverses orientation (a
relief in a bore). Closedness is therefore a property of the indexing, with
no weld, search or boolean, and the cost is one pass over the grid: a
million triangles in a fraction of a second. A full turn shares its seam
column; a back that is straight in `v` (a plane's, a cylinder's) is one
strip of long quads fanned onto the side walls at the open ends, so it adds
no T-junction. The values are settled by the script layer before the op —
a picture decoded and resampled, a function called per node — and the op is
keyed by a hash of them.

### Tessellation

`cadpole_core::brepmesh` polylines every edge once, with one vertex id per
sample, and triangulates each face in its own parameter domain with
`cadpole_core::cdt`, a dependency-free constrained Delaunay triangulation
with exact integer predicates whose constraints are the edges' shared
samples. Seams are sampled once and inserted twice under the same ids, and a
pole is one id, so the mesh is closed by construction rather than welded
afterwards. `MeshedSolid::unmatched_edges` judges the result on the ids. A
face that cannot be triangulated is a diagnostic naming the face, its kind,
its box and the reason.

Interior sampling is per surface (`cadpole_core::ksurface`). A recognised
cone or surface of revolution is divided as a revolved curve
(`kernel::with_cones_revolved`, `kernel::revolution_of`), so a cone band is a
strip of two triangles per rim segment; a plane has no interior samples; and
every other spline patch is divided per direction and per knot span until
both the chordal sag is within tolerance and the normal turns by at most 8°
across a cell. Only tessellation sees these substitutions: the model keeps
the original surface.

### Deadlines and limits

`cadpole_script::limits` bounds an evaluation: a wall-clock `Deadline` (off
in the library, 20 s in `cadpole serve`, 120 s in the CLI unless
`--eval-budget-secs` says otherwise), a step count, a
recursion depth, a heap cap, a cap on `print` lines, and source nesting and
`load()` depth caps. The deadline is polled by the Starlark evaluator and
checked at the top of every Cadpole builtin, so it bounds a script to "budget
plus one kernel operation": nothing interrupts a native operation that is
already running. On wasm there is no clock, so the deadline never fires
there; the other limits still do, and the browser's answer to a runaway
script is to terminate the Worker. An overrun is a diagnostic, never a change
of kernel. `docs/security.md` has the full table.

## 3D-printing checks

`cadpole-print` runs on the tessellated body, so a B-rep and a mesh body go
through the same code. Each check returns positions, not a boolean.

- **Solid.** `cadpole_core::solid::validate` returns a report: closed (an
  edge on one triangle is a hole, on three or more a non-manifold seam),
  consistently oriented (opposite traversal on every edge, positive signed
  volume), embedded (no two triangles pass through each other, and no two
  coplanar triangles share area), plus the degenerate-triangle count,
  connected components, Euler characteristic and genus. The verdict is closed
  ∧ oriented ∧ embedded. The print check clusters the report's positions into
  findings and adds an error-level `solid` line, so a body that is not a solid
  fails `cadpole check`. `assert_solid(body)` asks the same question from a
  script.
- **Wall thickness** by ray casting: from each triangle's centroid, one ray
  down the inward normal plus four on an 18° cone, to the first surface that
  faces away along the ray.
- **Overhang** as `asin(n · −up)` per triangle (0° a vertical wall, 90° a
  flat ceiling), with faces resting on the plate excluded from the unsupported
  total. A bridge is not an overhang: a flat downward face held up at both
  ends whose shortest span between supports is at most `max_bridge` (10 mm by
  default) is reported as a bridge and left out of the total.
- **Support rules** (`cadpole-print`'s `support` module and the overhang
  check, `SupportRules` on the profile): the overhang left after bridges
  is joined into connected patches and judged by the largest against
  `max_overhang_area` (4 mm²); a near-level ledge held up on one side may
  stick out `max_cantilever` (3 mm); an **island** -- a region whose lowest
  point is in mid-air, found by sweeping the welded surface bottom up and
  joining each vertex to the lower neighbours already passed -- may hang
  no deeper than `min_island` (a layer); a round horizontal hole (a
  recognised horizontal cylinder, concave, half a turn or more) wider than
  `max_horizontal_hole` (7 mm) with a round top wants a teardrop; and from
  horizontal sections, a feature under `slender_width` (3 mm) taller than
  `max_slenderness` (8) times its width, and a part taller than
  `max_height_to_base` (6) times the root of its first-layer area, are
  warnings. A downward face within two layers of an upward face of the
  same part rests on it (a print-in-place gap). `Check::is_support` names
  the findings that mean "needs support", which `assert_support_free` and
  `cadpole check --support-free` judge.
- **Bed fit** as the bounding box in the printer's frame, also tried turned
  90° on the plate and turned across the plate diagonal (the footprint's
  convex hull, turned and measured, so a part too wide for the diagonal is
  not offered it), with the excess per axis. A round plate is tested against
  the footprint's smallest enclosing circle.
  When a body does not fit, the diagnostic ends with the literal `split(...)`
  call to write, which the report also returns as `split_advice`.
- **Orientation** (`orient_for_print`): the six axes plus the normals of the
  largest flat faces, scored `1.0 · (1 − unsupported/total) + 0.6 · (bed
  contact / best) + 0.3 · (1 − height/tallest)`, minus 10 for not fitting the
  machine, ties going to leaving the model alone.

The per-triangle thickness and overhang fields are the same arrays the
`thickness` and `overhang` analysis views colour-map (`docs/views.md`), so the
picture and the report cannot disagree.

**Printer profiles.** `cadpole_print::printers::PRINTERS` is the one table of
machines, each with its build volume, bed shape, nozzle, stock layer height,
material and a `source` string saying where the numbers came from. A profile
is a starting point: `print_check(body, printer="prusa_mk4", nozzle=0.6)` is
that machine with a different nozzle. Material is a name plus a density, so
the report carries `mass_g` (solid, an upper bound) and `layers`.

**`split()`.** Cutting a body with a plane is intersecting it with each of
the two half-spaces; cutting it with another body is `a & b` then `a - b`.
Both go through `Context::boolean_pair`, so a split routes between the two
kernels by the rules above and names its faces the same way: a surviving face
keeps its name, a divided face becomes `name#k`, and the face the cut exposed
is `<feature>/cut`. The pieces come back ordered along the plane normal,
negative side first. truck cannot always cut through curved faces, so a split
across a bore may fall back to the mesh kernel.

## Rendering

| Backend | Code | Where | Purpose |
| --- | --- | --- | --- |
| CPU rasterizer | `cadpole_core::render` | native and wasm, no GPU | deterministic headless PNGs for sweeps, thumbnails, CI and assistants |
| CPU path tracer | `cadpole_core::render::trace` | native and wasm, no GPU | final-quality headless images |
| GPU (wgpu) | `cadpole-render-wgpu` | native, headless | `cadpole render --backend gpu` |
| Web viewer | three.js on WebGPU, `src/web/src/viewer.ts` | browser | the interactive viewport, picking, selection highlight |

`RenderRequest` (`engine.proto`) selects `RENDER_BACKEND_RASTER` or
`RENDER_BACKEND_PATH_TRACE`; on the CLI that is `--backend raster|path`.
`--backend gpu` is a host choice with no proto value: it runs
`cadpole-render-wgpu` locally and falls back to the rasterizer where there is
no adapter. The render keywords, views, modes and overlays are documented in
`docs/views.md`.

The two CPU backends share the camera (`View`), the material contract
(`Material`), the light rig (`shading::Lights`), the procedural textures and
the ACES tonemap, so they are one look at two costs.

### The CPU rasterizer

The driver is fixed. For each anti-aliasing sample it jitters the camera by a
sub-pixel offset, rebuilds a per-pixel G-buffer (`Frame`: depth, normal,
position, item, face, coverage, AO, ground, HDR, display) and runs the stages
in order:

1. opaque geometry (`render/aa.rs`): tiled and threaded, strict `z < depth`
   with first-submitted-wins ties;
2. SSAO and the ground contact shadow (`render/occlusion.rs`);
3. shading into scene-linear HDR (`render/shading.rs`);
4. order-independent translucency (`render/oit.rs`);
5. tonemap (ACES fitted, exposure) into display-linear;
6. the line overlay (`render/lines.rs`): grid, feature edges, silhouettes and
   highlights as ink.

The resolve averages in display-linear (never in sRGB), adds an ordered
dither at quantization and encodes sRGB8. Uncovered pixels carry the
background bit-exactly through every stage, so backgrounds and contact sheets
stay byte-identical. `RenderOptions::draft()` (1 sample, no AO, no contact
shadow) is the sweep default.

Determinism is by construction: tiles and row bands write disjoint memory and
depend only on their inputs, so the output is byte-identical for any thread
count (`RenderOptions::threads`, 0 = every hardware thread; wasm is always
1).

### The GPU renderer

`cadpole-render-wgpu` (`wgpu` and `naga` 29, shaders in WGSL) has a headless
entry point: `Renderer::render` takes the same `(items, camera, options)` as
`cadpole_core::render::render` and returns the same `Image`, rendered into an
offscreen texture and read back. A GPU render and a CPU render of the same
scene can therefore be diffed in a test on a software adapter. Its passes
mirror the CPU rasterizer's stages one for one. Anti-aliasing is hardware MSAA
rather than jittered camera passes, with the same display-linear resolve.
Translucency keeps only the nearest layer. `Renderer::pick` reads back an id
pass. The debug and analysis views are not drawn on this backend; `--backend
gpu` falls back with a note.

### The CPU path tracer

`cadpole render --backend path --samples N --bounces N` runs
`cadpole_core::render::trace`.

**The integrator.** Unidirectional path tracing with next-event estimation
and multiple importance sampling (power heuristic, beta = 2), over a
binned-SAH BVH of every triangle in the scene (`render/bvh.rs`: a flat array
of 32-byte nodes, iterative front-to-back traversal, and an `occluded` variant
that stops at the first hit for shadow rays). The ray/box test is written in
SSE2 intrinsics on x86-64 and is bit-identical to the scalar form
(`the_sse_box_test_is_the_scalar_one`). Russian roulette starts after the
fourth bounce with a floor of 0.25 survival; `--bounces` defaults to 8; a
firefly clamp applies to anything that has already scattered once.

Next-event estimation picks which light its one shadow ray goes to by power,
not uniformly. The two places that reconstruct the same density for MIS use
the same probabilities
(`next_event_estimation_integrates_the_rig_to_its_irradiance`).

**Determinism.** Every pixel is a pure function of `(x, y, sample, seed)`.
The sampler is a Halton sequence Cranley-Patterson-rotated per pixel by a hash
of the pixel index; there is no shared RNG state and no accumulation order to
depend on. The BVH build is sequential. Work is split into row bands handed
out from a queue, so the image is byte-identical for any thread count, and
wasm32 (one thread) produces the same pixels as a multi-threaded desktop.

**Print materials.** `Material` carries five fields the rasterizer ignores,
all defaulting to 0 = off:

| Field | What it does |
| --- | --- |
| `transmission` | GGX microfacet transmission, so a clear part refracts by Snell's law instead of alpha-blending. `0` on a translucent material means "derive from `opacity`". |
| `attenuation_distance` | Beer-Lambert absorption inside the solid, tinted by `base_color`: transmittance over that distance *is* `base_color`. A thick section of translucent filament is darker and more saturated than a thin one. |
| `scatter_distance`, `scatter_g` | A random walk inside the solid with a Henyey-Greenstein phase function: milky filaments and frosted resin. Next-event estimation runs at every scattering event. |
| `anisotropy` | The specular lobe is stretched up the layer-stacking direction and narrowed along the extrusion (`ax = alpha(1 - 0.7k)`, `ay = alpha(1 + 3k)`, `k = anisotropy * (1 - |n.z|)`), so an FDM wall reads as printed. It fades to nothing on faces whose normal is Z. |

Emissive surfaces are light sources, not just bright pixels: every emissive
triangle goes into a power-weighted CDF, next-event estimation samples it by
area, and the BSDF strategy that hits it is MIS-weighted against that.

**Guide geometry** (`Material.shadow`) is a rule about transport rather than
a look: a body with it false is met by the camera's specular chain from the
eye and by nothing else. Shadow rays, bounces and the emissive light list all
skip it (`Bvh::intersect_where`, `occluded_where`, `shadow_hits_where`). The
rasterizer honours the same flag by leaving the body out of the sun shadow
and the contact heightfield.

**Two choices that are about noise, not physics.**

1. *A smooth dielectric splits the path rather than tossing a coin.* A clear
   interface reflects a few percent and refracts the rest, and those two
   halves see very unequal things. A Fresnel-weighted coin flip is unbiased
   but noisy, so both branches are traced, up to eight splits per camera ray.
   Anything whose microfacet alpha is at or below 0.02 takes this path;
   frosted and translucent materials keep their wide lobes.
2. *No caustics after a diffuse bounce.* Light that reaches the eye by
   bouncing off something matte and then through a chain of mirrors or clear
   walls gets MIS weight 0. It is already in the image, because a shadow ray
   passes through a clear part with its transmittance. What this gives up is
   the sharp focused caustic a glass ring throws on a table.

**Tests are physics, not screenshots** (`render/trace.rs`, `render/bvh.rs`):
a white furnace converging to the environment radiance; energy conservation
across the roughness × metallic grid; Snell's law, and a glass sphere
inverting the background it refracts; Beer-Lambert measured against
`base_color^(d/attenuation)` per channel on two slab thicknesses; a rough
dielectric blurring a stripe pattern a smooth one keeps; colour bleeding in a
Cornell-style box; an emissive panel measurably lighting a wall, with falloff;
BVH traversal agreeing with brute force on a random scene; and byte-identical
images across thread counts.

**Limitations.** GGX here is single-scattering, so a rough metal in a furnace
loses energy (a polished one loses very little). There are no caustics. The
line overlay and the infinite grid are rasterizer-only: the path backend has a
real floor and real contact shadows instead.

### Debug views

`cadpole render --debug faces|normals|defects|seams|wire` draws the mesh
rather than the part (`docs/views.md`, "Debug views").

Four of them are colours per triangle (`Item::tri_colors`, with the
material's character turned off): `faces` is a golden-ratio palette over the
face index (`render::debug::face_color`); `defects` is `solid::validate`'s
crossings mapped back to the drawn triangles through `Mesh::welded_with_map`,
plus slivers and folds; and `seams` and `wire` paint a flat grey for their ink
to sit on (`lines::seams_overlay`, `lines::wire_overlay`). `normals` is per
pixel (`shading::shade_debug`), which is also where every debug view gets its
light: one directional headlight along the view axis, with no environment,
specular or occlusion. It is directional rather than a point light at the eye
so that a flat face is one colour under perspective.

`RenderOptions::set_debug` forces what a measurement needs: the rasterizer,
one sample, no AO or contact shadow, no tonemap, unit exposure, no x-ray; the
engine forces opacity 1 on every item.

## Session and event protocol

One `cadpole serve` holds one session for one document. Its clients are the
web UI (possibly several tabs), the CLI, the Apple apps and an assistant
(through `cadpole mcp`). `spec/session.qnt` and the other models under `spec/`
describe the protocol; `spec/README.md` lists them with their invariants.

Rules:

- `revision` is a monotonic counter over *content* changes: the script files
  and the parameter overrides. Selection, view state and messages do not bump
  it.
- Writing a script requires `base_revision`. If it does not match, the write
  is refused and the caller re-reads and rebases. `SetScript` and
  `EditScript` answer with `conflict = true` and the session's current
  `revision` and `source` in the response; the hand-written
  `PUT /api/session/script` route answers `409 Conflict`. This is optimistic
  concurrency, which is enough because writers are few and human-paced.
- A script that does not parse is returned to the caller as an error and is
  not stored.
- Parameter overrides and selection are last-writer-wins.
- Selection refs survive script and parameter writes: the server stores them
  verbatim and only a selection write changes the set. Whether a ref is live
  or stale is derived by resolving it against the current model on every read
  (see [Selection refs and staleness](#selection-refs-and-staleness)), never
  remembered, so a ref whose feature is removed and later restored is live
  again.
- Every change appends an `Event` with a strictly increasing `seq`. Event
  kinds are `script_changed`, `params_changed`, `selection_changed`,
  `message`, `assistant_listening` and `client_failure`. Delivery is
  at-least-once and clients dedupe by `seq`.
- Messages are an append-only chat between `user` and `assistant`. The user's
  client posts with `PostMessage`, an assistant with `Reply`: `from` is
  decided by the operation, never by a field the caller sets, and
  `PostMessage` is denied to the limited listener for that reason.
- `Save` writes the session's document to the path `cadpole serve` was
  started with. It takes no path and can write nowhere else; the session
  itself is not persisted, and a restarted server starts a new one with a new
  id.

### Writer identity and cursors

- Every writer identifies itself with an `X-Cadpole-Client: <id>` header.
  `Event.by` is set from it, and `WaitEvents` and `/api/events/poll` never
  return a client its own writes.
- Cursors live on the server. `Ack {client, cursor}` stores a per-client
  cursor that only moves forward, and `WaitEvents` with `after: 0` resumes
  from it. `cadpole mcp` acks after every poll batch and resumes from the
  stored cursor at startup, so a message sent while the assistant's process
  was restarting is still delivered.
- A cursor past the end of the log means the server restarted. It comes back
  with `epoch_reset: true`; the client discards the cursor and re-reads.
- `ChatMessage.seq` is its own counter, separate from `Event.seq`.
  `GetAttachment` takes the *message* seq, and the notification an assistant
  receives carries it as `message_seq`.

### Attachments

A message from the UI can carry a screenshot of the viewport.
`GetAttachment {seq, index}` returns its `png`, `kind`, `annotated`, `width`,
`height` and the `camera` it was taken with. `(cadpole.op).image_field` turns
the PNG into an MCP image block, so an assistant that has no HTTP client can
see the picture and the base64 never enters a text block. `GetSession` does
not return attachment bytes.

### Hover

The UI writes `{selected, hovered, note}` as one selection resource, on every
hover change. The server emits `selection_changed` only when the selected set
or the note changed, so moving the mouse across a model does not push an event
per face to an assistant. `GetSelection` returns `hovered` on demand.

### Client failure reports

The event log also carries what the front end failed to do.
`ReportClientFailure` turns one report into one `client_failure` event and one
line on the server's stderr. The browser feeds it from five places:

| Source | Caught by |
| --- | --- |
| a wasm panic | `cadpole-wasm`'s own panic hook, which keeps the Rust message and `file:line:column`; the page reads it back with `take_panic()` |
| an uncaught exception | `window.onerror` |
| an unhandled rejection | `unhandledrejection` |
| a worker | the worker posts to the page and the page forwards it (`worker-report.ts`), because neither handler above is scoped to a worker |
| an operation error | the `catch` around an evaluate, render, save or session sync |

A report carries the message, a stack, the operation, the script revision,
the effective parameters, a digest of the script, the engine build and kind,
and the surface it came from. `Session.client_failures` counts them, so
`GetSession` is enough to notice, and `WaitEvents(after: 0, kinds:
["client_failure"])` reads them back. The size caps and the reason the
listener may not call it are in `docs/security.md`, "Client failure reports".

## Parameter sweeps

`sweep` is the parameter study; the geometric operation is `sweep_along`.

`cadpole sweep main.star --param w=30,40,50 --param h=10,20 -o sheet.png`
evaluates the cartesian product of the listed values, renders every cell with
one shared camera on the CPU rasterizer, and composes a labelled contact
sheet. `SweepRequest.param_order` says which parameter varies fastest (the
last one listed becomes the columns). The response is the sheet plus a typed
index, so an assistant can read the image, pick a cell and apply its values as
overrides without parsing a label back into numbers:

```proto
message SweepResponse {
  bytes png = 1;
  uint32 width = 2;
  uint32 height = 3;
  uint32 cols = 4;                       // cells per row actually used
  repeated SweepCell cells = 5;          // sheet order: left to right, top to bottom
  repeated Diagnostic diagnostics = 6;   // about the request itself
}
message SweepCell {
  map<string, ParamValue> params = 1;    // the overrides for this cell
  string label = 2;                      // as drawn, e.g. "wall=1.6 height=80"
  BBox bbox = 3;
  repeated Diagnostic diagnostics = 4;
  bool ok = 5;                           // false: an error diagnostic; the cell is drawn empty
}
```

Cells are evaluated in parallel natively (wasm is serial). A sweep is refused
above `max_cells` (400 by default); a server applies its own limits on top
(`docs/security.md`, "Render and sweep limits").

## Assemblies

Display assemblies place bodies without solving anything:

```
p = part(body_or_bodies, *, name)                 # built once; the bodies stop rendering on their own
i = instance(p, *, at, rotate, name, material)    # a pose: shares p's geometry, tessellation and names
place(i, frame_on_i, frame_on_target, *, flush, offset)   # frames from named faces/edges/vertices
pose(i, *, translate, rotate)                     # a parameter-driven displacement
a = assembly([instances, assemblies], *, name)
explode(a, distance, *, along)                    # view-time offsets, ranked along an axis; never geometry
```

`docs/parts.md` has the full signatures.

`Model` carries `parts`, `instances` and `assemblies`, and `Body` carries
`part_body`, `transform` and `instance`. A part body's mesh goes out once,
invisible, and every instance is a pointer at it with a transform (the pose
plus the exploded-view offset), so re-posing is sixteen numbers and the web
viewer shares one geometry per part. An explode is a script record, usually
driven by a `param`, and the view offset is derived from it per scene, so a
render, a sweep and a test can all vary it. `examples/planetary.star` is the
worked example.

The same shape carries print plates: `Model.plates` lists every plate of the
document's Bambu projects with its bed and, per body on it, the transform the
export gives it and its filament slot and colour. It is placement only,
computed by the layout function the 3MF writer uses, so the web UI's plates
view draws the meshes it already has.

Not implemented: mates, joints, a constraint solver between parts,
interference checks and a bill of materials.

## Operations, CLI and crates

### One operation registry

Every user-facing operation is defined once, as a method of
`service CadpoleOps` in `proto/cadpole/v1/ops.proto`: request and response
messages, the rpc's leading comment as its documentation, and field comments
as parameter help. `proto/cadpole/v1/options.proto` defines the `(cadpole.op)`,
`(cadpole.field)` and `(cadpole.type)` options that shape the front ends.
`build_support/proto/defs.bzl` builds the descriptor set with
`--include_source_info --include_imports` (the comments are the
documentation) and embeds it in a Rust crate, and `cadpole-ops` exposes
`registry()`, `json_schema()`, `call_json()` and `call_proto()` over it using
`prost-reflect`. Every front end is derived from the descriptor at run time:

| Front end | Derivation |
| --- | --- |
| MCP (`cadpole mcp`) | one tool per rpc, named in snake_case; `inputSchema` is JSON Schema generated from the request message; the description is the rpc comment; a call is JSON → dynamic message → engine → JSON |
| HTTP (`cadpole serve`) | unary Connect at `POST /api/cadpole.v1.CadpoleOps/<Rpc>`, in protobuf or JSON, and gRPC-Web framing on the same route; `POST /api/ops/<Rpc>` is an alias and `GET /api/ops` lists the registry |
| CLI | `cadpole op <Rpc>` with flags generated from the request fields; `(cadpole.op).cli_name` is the ergonomic verb, with `alias` keeping other spellings working |
| Browser | `src/web/src/api.ts` calls one unary Connect rpc per operation in binary protobuf. The TypeScript messages and service descriptor are generated from the same descriptor set on every build. `wire.ts` is the only module that encodes anything, and `connect.ts` is the transport |

`mcp.rs`, `server.rs` and `op_cli.rs` hold no operation table of their own,
and the test `every_front_end_exposes_exactly_the_registry` asserts that the
name sets (registry, MCP tools, HTTP routes, `cadpole op` subcommands) are
equal. `src/rust/cadpole-cli/src/connect.rs` implements the unary subset of
the Connect protocol and gRPC-Web's framing over the existing router, so
dispatch, the auth guard and the capability check are shared by every route.
Full gRPC is not served.

Adding an operation is an rpc in `ops.proto` with its messages plus one arm
in `cadpole-ops`'s `handle`, then classifying it for the limited listener
(`src/rust/cadpole-ops/src/capability.rs`).

The operations, with the CLI verb each declares. "Session" marks the ones
that need a running `cadpole serve`; the others take a document in the
request.

| rpc | `cli_name` | Session |
| --- | --- | --- |
| `Evaluate` | `eval` | |
| `Render` | `render` | |
| `Sweep` | `sweep` | |
| `Animate` | `animate` | |
| `Export` | `export` | |
| `PrintCheck` | `check` | |
| `Plate` | `plate` | |
| `Inspect` | `inspect` | |
| `Drawing` | `drawing` | |
| `RunTests` | `test` | |
| `ApiDoc` | `doc` | |
| `ExplainRef` | `explain-ref` | |
| `ListAssets`, `GetAsset` | `assets-list`, `assets-get` | |
| `Solve` | `solve` | |
| `ListJobs`, `WaitJobs`, `CancelJobs` | `jobs`, `jobs-wait`, `jobs-cancel` | |
| `GetSession` | `session` | yes |
| `GetSelection` | `selection` | yes |
| `SetScript` | `set-script` | yes |
| `EditScript` | `edit-script` | yes |
| `SetParams` | `set-params` | yes |
| `SetSelection` | `set-selection` | yes |
| `Reply` | `reply` | yes |
| `StreamReply` | `stream-reply` (hidden) | yes |
| `PostMessage` | `post-message` (hidden) | yes |
| `Save` | `save` | yes |
| `WaitEvents` | `listen` | yes |
| `Ack` | `ack` | yes |
| `GetAttachment` | `attachment` | yes |
| `AddAsset` | `assets-add` | yes |
| `ReportClientFailure` | `report-failure` | yes |

Two details are handled in the registry rather than once per front end. A
response field named by `(cadpole.op).image_field` becomes an MCP `image`
block and `cadpole op --out FILE`, never base64 in a text block. A response
field marked `(cadpole.field).hidden` (such as `RenderResponse.model`) is left
out of the JSON the front ends print, while typed callers still get it.

`EditScript` exists so an assistant does not have to re-emit a whole file. A
`ScriptEdit` is one of three forms: `old_str`/`new_str` (each `old_str` must
be unique unless `replace_all` is set); a 1-based inclusive
`start_line`/`end_line` range; or a `feature` id whose call's argument list is
replaced, which suits a caller that has just read `ExplainRef`. Edits apply in
order and are all-or-nothing: if any edit does not apply, nothing is written
and the error names the edit. The parse happens before the store, and the
revision check is repeated under the lock. The response carries a unified
diff; on a conflict it carries the session's current source and a diff
against the caller's base.

`WaitEvents` is a long poll. `cadpole mcp` turns its batches into channel
notifications; the HTTP server also offers the same log as SSE at
`/api/events` and as a long poll at `/api/events/poll`; `cadpole listen`
prints JSON lines.

### Streamed replies

`Reply` posts a message whole. `StreamReply` posts one a piece at a time, so
a page shows an assistant's answer as it is written: the first call (`seq`
0) starts a message, later calls append text to it and set a one-line
`activity` ("rendering…"), and a call with `finish` ends it as done or
failed. It is what the limited listener calls with a model's output; it is
hidden from `tools/list` because it is not a tool for a model.

The rules are enforced by the server
(`src/rust/cadpole-cli/src/reply_stream.rs`):

| Rule | Detail |
| --- | --- |
| Attribution | `from` is `assistant`, set by the server. The request has no field for it |
| Author | Only the client that started a message continues it: the token's role and the `X-Cadpole-Client` id of the call must both match |
| Finality | A message that is `done` or `failed` never changes; every later call naming it is refused |
| Text | Append only. The text a caller appends to one message is capped at the message limit (64 KiB) |
| Activity | One line, at most 120 bytes, no control characters; cleared when the message ends |
| Rate | 8 calls back to back per message, then one every 200 ms; a call beyond that is answered `throttled` and changes nothing. The call that ends a message is never throttled |
| Concurrency | One message in progress per client (starting another ends the earlier one as failed) and four per session |
| Abandonment | A message with no accepted call for 30 s, or still in progress 30 min after it started, is ended as failed by the server with a closing line of its own. The server checks once a second |

**What travels where.** The event log (`WaitEvents`, `cadpole listen`, the
channel, `/api/events`) gains exactly one event per streamed message: a
`message` event when it ends, carrying the final text and `state`. That is
the event `Reply` produces, so a consumer that knows nothing about streaming
sees a finished reply and nothing else, and the event starts no turn because
it is not from the user.

The pieces do not go through the log. The log is a bounded window that every
reader resumes from by sequence number, and each entry ends every pending
long poll; a reply written over a minute would be hundreds of entries that
push real events out of the window and wake every watcher for text nobody
acts on. Instead each accepted, non-final call becomes a `reply_stream`
frame on a separate broadcast, sent only on an event stream opened with
`/api/events?after=<seq>&live=1`:

```
event: reply_stream
data: {"seq":0,"kind":"reply_stream","data":{"seq":12,"from":"assistant","at":"…","state":"streaming",
       "offset":17,"length":29,"append":"2 mm thicker","activity":""}}
```

A frame has no SSE id and its event `seq` is 0, so it never moves the
position a reader resumes from, and it is stored nowhere. Nothing is owed to
a reader that missed one: `offset` is where `append` starts in the message's
text, in bytes of UTF-8, so a page that sees a gap reads the session, whose
`messages` carry a message in progress with `state: "streaming"` and the
text so far. The web UI folds frames, session reads and the final event into
its message list in `src/web/src/reply-stream.ts`: text only grows, and a
message that has ended is not reopened by a late frame or a stale read.

The caller coalesces: the listener sends what arrived in the last 250 ms as
one call, and an empty call every 5 s while a tool runs.

`spec/reply_stream.qnt` states these rules as invariants, and its traces
are replayed against the handler (spec/README.md).

`src/rust/cadpole-ops/testdata/render_request.schema.json` is a checked-in
golden of one generated schema, so a proto edit cannot silently change what
assistants are told.

### The `cadpole` binary

`cadpole-cli` builds the one user-facing binary. `cadpole --help` and
`cadpole <verb> --help` are the reference for verbs and flags;
`docs/getting-started.md` walks through the common ones. Conventions that
hold across verbs:

- A document argument is a packed document or a `.star` script (a script
  alone is an unsaved document with defaults). `--param k=v` (`-p`) is
  repeatable wherever a document is evaluated.
- With `--json`, stdout is the JSON result; diagnostics and progress go to
  stderr.
- Exit code `0` is success, `1` means the document has errors (the output
  still describes them), `2` is a usage error.
- Three flags apply to every verb: `--max-asset-mb N` (`CADPOLE_MAX_ASSET_MB`),
  the largest asset a native run reads from `./assets` beside a script,
  `--max-heap-mb N` (`CADPOLE_MAX_HEAP_MB`), the Starlark heap one script may
  fill (512 MB by default), and `--eval-budget-secs N`
  (`CADPOLE_EVAL_BUDGET_SECS`), the wall clock one evaluation may take (120 s
  by default, 0 for none; a fine SDF mesh for a print can need more).
  `cadpole serve` keeps its own budget, `--eval-deadline-ms`.

`cadpole serve` mints a session token and a listener token on every start,
`0600` in a `0700` directory under `$XDG_RUNTIME_DIR/cadpole`. Every route
requires the token; `--no-auth` turns that off and is refused anywhere but
loopback. `docs/security.md` covers binding, tokens and capabilities.

### Crate boundaries

All crates live under `src/rust`. `docs/architecture.md` has the overall
shape; this is what each one owns.

| Crate | Owns | Depends on (Cadpole crates) |
| --- | --- | --- |
| `cadpole-proto` | generated protobuf types | none |
| `cadpole-core` | geometry, sketches, features, `Context`, both kernels, naming, tessellation, the CPU renderers, the memo; converts *to* proto types. Never sees Starlark, request envelopes, HTTP or files | proto |
| `cadpole-sketch-solve` | the 2D constraint solver | none |
| `cadpole-field` | signed distance fields and their meshing | core |
| `cadpole-print` | print checks, printer profiles, orientation | core |
| `cadpole-opt` | linear and mixed-integer optimisation models (`opt`) | core |
| `cadpole-font` | text outlines | core |
| `cadpole-assets` | asset storage and limits | none |
| `cadpole-import-2d`, `cadpole-import` | 2D and 3D importers | core, and the drawing and export crates |
| `cadpole-drawing`, `cadpole-drawing-out` | 2D drawings and their SVG, PDF and DXF writers | core, font, import-2d |
| `cadpole-export-3mf`, `cadpole-export-step` | the 3MF and STEP writers | core |
| `cadpole-script` | the Starlark API; drives `cadpole_core::Context`; API doc generation. Never sees request envelopes or HTTP | core and the feature crates above |
| `cadpole-engine` | the `Request`/`Response` envelope, document → model, the JSON session view, sweeps and contact sheets, exports. The only crate that knows the envelope | core, script and the feature crates |
| `cadpole-ops` | `service CadpoleOps` as a registry; capabilities | core, engine, proto |
| `cadpole-render-wgpu` | the GPU renderer; sees meshes, not features | core |
| `cadpole-cli` | verbs, files, the HTTP server and session, MCP | engine, ops, render-wgpu |
| `cadpole-wasm` | `wasm-bindgen` glue and typed-array views | engine |
| `cadpole-ffi` | the C ABI: `cadpole_call(bytes) -> bytes` and friends | engine |
| `cadpole-bench` | a development benchmark binary | engine |

A feature is implemented in the lowest crate that can express it, exposed in
`cadpole-script`, documented in `docs/starlark-api.md`, and then reaches
every platform. A shell (`cli`, `wasm`, `ffi`) contains no geometry. A
dependency of `cadpole-core` or `cadpole-script` has to build for wasm32.

### The kernel-op memo

Nothing survives an evaluation except what the caller keeps, and what the
caller keeps is one thing: `cadpole_core::memo::Memo`, a bounded store in
front of the kernel. `EvalInput.memo` is `Option<Arc<Memo>>`, shared and
immutable for one evaluation.

- `cadpole serve` holds one per session and hands it to every evaluation of
  the session's document (`cadpole_ops::Host::memo`).
- The wasm engine holds one for its lifetime.
- `RunTests` shares one across its sweep cells and `Animate` one across its
  frames.
- A one-shot `cadpole eval` passes none.

The script always runs from the top; Starlark is a small part of the cost.
Every kernel operation in `Context` (extrude, revolve, loft, cone, the
booleans, the merged pattern, glue, split, the fused mirror, sphere, torus,
hull, and every display tessellation) goes through `Context::memoised` with a
key of canonical bytes: the kernel version, the operation, the feature id its
names are minted under, the operands' identities (SHA-256 of what made them,
a Merkle chain), every numeric input as `f64` bits, and the routing inputs
(both tolerances, the face budget, the mesh engine, the snap switch). Keys
are compared in full. A hit returns the operation's bodies with their name
sources and replays its diagnostics; a miss runs the operation and stores it.

The store is LRU by bytes (256 MB natively, 96 MB in wasm), cleared when the
kernel version moves, never persisted, and never shared between a server and
a browser. A drag preview (`EvaluateRequest.preview`) defers every fuse whose
key the memo does not hold and shows its operands; the release runs them.
`Model.memo` reports the counts and `Feature.cached` whether every operation
of a feature was a hit.

## Heavy work in the browser

Every heavy operation in the web UI is a job in a named slot
(`src/web/src/jobs.ts`). A slot runs one job at a time, starts them in
submission order, and has one of three policies saying what a second request
means:

| Policy | Meaning | Used by |
| --- | --- | --- |
| `supersede` | the newer request makes the older worthless: the same question with different inputs | `evaluate` |
| `coalesce` | the newer request is the same question, so it joins the running job | `apiDoc` |
| `queue` | every request must happen, in order, none dropped | writes such as `save` |

Cancellation is always requested; whether it reclaims CPU depends on the
engine. Either way the runner guarantees that a cancelled job's result is
never delivered, so nothing downstream re-checks whether what it holds is
current.

The engines sit behind one interface, `EngineLike`, whose `evaluate` is
asynchronous and returns the model *and its meshes* as one value rather than
engine state a late result could overwrite.

| Engine | Runs | When |
| --- | --- | --- |
| `WorkerEngine` | the wasm engine in a dedicated module worker | the default |
| `RemoteEngine` | the `Evaluate` operation on `cadpole serve` | when the policy promotes the document |
| `WasmEngine` | the wasm engine on the main thread | no `Worker`, or `?engine=main` |
| `MockEngine` | a hardcoded model | `?mock=1`, or the wasm will not load |

The worker is the default because it is the only one that can actually be
stopped, and because it needs no server.

**Local or remote is measured, remembered per document, and overridable**
(`src/web/src/engine-policy.ts`). `Auto` evaluates locally first, promotes
the document to the server once a local evaluation has exceeded 750 ms, and
demotes it again if remote turns out not to be faster end to end. The status
bar names the engine that ran, and the toolbar's Engine control overrides.
Remote is offered only when the page is attached to a session.

**Falling back never leaves a dead UI.** An unreachable server, a 401 after a
restart, a timeout or a server error all re-run the same input locally, once,
demote the document, and say so in the status bar and in a `client_failure`
report.

**A job produces a sequence, not a value.** Evaluation uses that to show a
coarse mesh (0.4 mm tolerance) before the requested one when measurement says
the coarse pass arrives much sooner. That is the mesh-kernel case, where
tolerance feeds the booleans; for an exact B-rep the tessellation is cheap
and a coarse pass would cost a whole extra evaluation. A refinement does not
move the camera or disturb the selection, because what is selected is a
topological name rather than a triangle.

`?perf=1` makes the apply path name its own phases.

## The limited-privilege listener

`//src/tools/listener` is a process that sits beside `cadpole serve` and
watches one session, so a user can select a face, type a note and get an
answer without an interactive assistant session open.

```
cadpole serve ──────────────────────────────── the session
   │  mints two tokens per run
   │    session-token   → the UI, cadpole op, an interactive cadpole mcp
   │    listener-token  → a subset of the operations, enforced in the server
   │
   ├── the web UI              Authorization: Bearer <session-token>
   │
   └── cadpole mcp --listener  Authorization: Bearer <listener-token>
            ▲                    ▲
            │ stdio (MCP)        │ stdio (MCP), spawned by the headless process
            │                    │
     //src/tools/listener ───► claude -p (headless) ───► the model
       the event loop            (behind a seam)
```

- **The listener process holds no token.** It drives `cadpole mcp --listener`
  over stdio and reads events through the `WaitEvents` operation, so the
  credential is found by the child, in the child, and the loop has no HTTP
  client of its own.
- **Its privilege is a capability on the token, not a list in the client.**
  The server refuses a denied operation with `403` before any handler runs,
  so a listener that has been prompt-injected through a selection note still
  cannot call `SetScript`. `docs/security.md`, "Capabilities", has the table
  of granted and denied operations with the reason for each.
- **Only a note or a message starts a turn.** A bare click is not a request.
  The UI's Listen toggle gates the whole thing and is read from the event
  stream, so turning it off takes effect on the next event.

The listener is ESM JavaScript run by Node. The model is behind a small seam
(`driver.mjs`): the real driver keeps one headless process of the installed
Claude Code per conversation and writes each event to it, with the Cadpole
tools as its only tools, and `--driver echo` runs the entire loop against a
real server with no model. Nothing runs while no event arrives.
`docs/assistant-integration.md` covers setup and policy.

## Testing

| Layer | What | Where |
| --- | --- | --- |
| unit | geometry, sketches, kernels, naming, context, renderers, each front end | a `rust_test` per crate: `bazel test //src/rust/...` |
| script tests | `test(name, fn, *, sweep=, monotone=)` declarations in `.star` files, run after the model is built, with `assert_solid`, `assert_volume`, `assert_bbox`, `assert_printable`, `assert_close` | `cadpole test`; `every_example_test_passes` in `cadpole-script` runs every test the examples declare |
| render physics | furnace, energy conservation, refraction, absorption, BVH against brute force, thread-count determinism | in `cadpole-core` (see [The CPU path tracer](#the-cpu-path-tracer)) |
| registry | the front ends expose exactly the registry; a golden JSON Schema | `cadpole-cli` and `cadpole-ops` tests |
| protocol | Quint models of the session protocol, the event log, revisions, the browser's job seam, the reload loop, the FFI deadlines and the capability boundary. `//spec:names_test` diffs the models' operation lists and spellings against the Rust and the proto | `//spec:quint_test`, `//spec:names_test`; `bazel run //spec:quint_check` is the same check with more samples |
| web end to end | Playwright against the built UI: a mock engine with committed screenshot baselines, the real wasm engine, and a real `cadpole serve` | `//src/web:e2e_test`; `bazel run //src/web:e2e_update` regenerates the baselines |
| listener | the event loop against a fake MCP server, and against a real server | `//src/tools/listener:listener_test`, `:listener_e2e_test` |
| supply chain | licences of every crate and npm package against an allow-list; RustSec and OSV advisories from vendored snapshots; the generated `THIRD-PARTY-NOTICES.md` | `bazel test //build_support/...` |

Quint and the Playwright browser are pinned and fetched by Bazel, so a
missing tool is a build error rather than a skipped test.

**What determinism guarantees.** Feature ids, topological names, face, edge
and vertex counts and diagnostics are the same on every platform.
Coordinates agree closely but not bit for bit between native and wasm,
because transcendental functions differ in their last bits per platform. The
CPU renderers are byte-identical across thread counts on one platform.

Examples under `examples/` are test data: every example is evaluated by the
test suite.

## The 2D constraint solver

`cadpole-sketch-solve` is a standalone crate with no dependencies. It builds
for wasm32 unchanged.

**Model.** Entities sit over one flat parameter vector, parameterised as
tightly as possible because every parameter is a degree of freedom the user
has to constrain away. A point is `x, y`. A line owns nothing and references
two points, so two lines sharing a corner share it exactly. A circle is a
centre point plus `r`; an arc is a centre point plus `r, start, end`.
Anything point-like (a line end, an arc end, a circle centre) is a `PointRef`,
and a reference carries its own derivative, so one implementation of
`coincident` works on an arc endpoint at `centre + r·(cos θ, sin θ)` and moves
the arc's angle to suit. Any entity can be flagged as construction geometry.
Every constraint contributes residual rows with analytic Jacobians; nothing
is differentiated numerically.

**Solver.** Gauss–Newton with Levenberg–Marquardt damping inside a Powell
dogleg trust region. The trust region is what survives a bad initial guess:
the step is bounded by a radius that grows when the linear model predicts
well and shrinks when it does not. The damped normal equations
`(JᵀJ + λI) h = −Jᵀr`, solved by Cholesky, give the Gauss–Newton step. At λ's
floor (10⁻¹² of `‖JᵀJ‖`) that is the minimum-norm step, because a direction
the Jacobian cannot see is damped to nothing rather than amplified. That is
what makes an under-constrained sketch move as little as it can. Residual
rows are weighted by unit before they reach the solver (a length as itself,
an angle times the sketch's characteristic size), so a millimetre and a
radian converge together. There is no RNG, no hashing and no timing: the same
sketch and the same starting values always give the same answer, and the
answer is the root nearest where the geometry already was.

**Diagnostics.** One rank-revealing orthogonalisation of the Jacobian's rows
(modified Gram–Schmidt with a reorthogonalisation pass, which is a QR of
`Jᵀ`) answers three questions:

- **Degrees of freedom** = parameters − rank. A numerical rank at a generic
  configuration catches the dependences a structural count misses, such as
  the third angle of a triangle or the fourth side of a rectangle.
- **Redundant and conflicting constraints, by name.** Rows are taken in the
  order the user added them, so a dependent row is blamed on the constraint
  added last. Each basis vector is kept as an explicit combination of the
  rows that built it, so the coefficients that reproduce a dependent row name
  the constraints responsible. Redundant or conflicting is then one dot
  product: the dependence is a vector `z` with `zᵀJ ≈ 0`, and the group is
  jointly satisfiable only if `zᵀr ≈ 0` too; `zᵀr` is the size of the
  contradiction in millimetres. The message reads
  `redundant: top width is already implied by width, vertical#3, vertical#4`
  or `conflicting: top width contradicts width by 5.000000`.
- **Which entities are still free**: the part of each unit axis lying outside
  the row space.

**Dragging** (`System::drag`) pins the dragged point with a temporary
constraint at a fifth of a real constraint's weight, solves, then drops the
pin and solves again. The first pass tracks the cursor along every direction
the sketch leaves free; the second is a minimum-norm projection back onto the
constraint manifold, so the constraints end up satisfied exactly and a point
constrained onto a circle follows the cursor around the circle. The result
reports whether the cursor was reached and by how much it was missed.

**In Starlark.** `constrained_sketch()` is the surface
(`docs/starlark-api.md`): named points, lines, arcs and circles,
`constrain(kind, ...)`, and construction geometry that shapes the solve
without becoming material. Everything is named because every diagnostic
prints those names. Building is lazy: the solve happens when the sketch is
consumed (extruded, revolved or queried) and is cached until the sketch
changes. The result is an ordinary `Sketch`, chained from the solved curves
by position, so nothing downstream knows the difference. The verdicts reach
the script author as diagnostics with the span of the `constrain()` call
that caused them: remaining degrees of freedom as *info*, a redundant
constraint as a *warning* naming what already implied it, and a contradiction
as an *error* naming both sides and the gap in millimetres, which also stops
the operation rather than extruding a least-squares compromise.

**Limitations.** Cost grows roughly cubically with sketch size and there is
no decomposition into connected components. There is no spline entity.
`tangent` between two curves that already touch needs the explicit third
argument, because the distance residual is stationary there. Loop chaining is
geometric and greedy, so a sketch where three curves meet at one point needs
`close([...])` to say which two belong together.
