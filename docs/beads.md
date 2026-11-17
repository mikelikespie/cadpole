# Bead scenes

`cadpole render-beads` path-traces geometry that is not a solid model:
thousands to millions of swept beads, each the solid a round nozzle leaves
along a straight move. Extruded plastic from a toolpath is the case it was
built for; wire, tubing and piping read the same way. There is no script and
no document: a JSON file names a binary bead file, the materials, a few
boxes to stand the beads on, the camera and the render settings.

```
cadpole render-beads scene.json -o print.png
cadpole render-beads scene.json -o thumb.png --size 640x640 --samples 16
```

Timings and sizes go to stderr as one JSON line (`beads`, `bvh_nodes`,
`bead_mb`, `build_ms`, `trace_s`, `mean_samples`, ...). The command reads the two files it is
given and nothing else; it is a command-line tool, not a server operation.

## The bead

A bead from `a` to `b` is a capsule whose cross-section is an ellipse `2 rx`
wide and `2 rz` tall, flattened along world Z. From above it is a stadium with
round ends, so the beads of one path join without a notch at any corner; seen
end-on it is the ellipse. It is the image of a round capsule of radius `rx`
under the scaling `diag(1, 1, rz / rx)`, so a ray is intersected with a
quadratic and the normal is exact. A bead with `rx == rz` is a round capsule
in any direction, which is how a vertical rod or a wire is drawn.

For an extruded line `w` wide in a layer `h` tall, a half width a little over
`w / 2` and a half height about `1.25 * h / 2` make neighbours overlap the way
squashed plastic does: the grooves between layers are about as deep as a real
print's, and the top of a solid layer is nearly flat with a fine ridge per
line.

## Files

The bead file is little-endian: the eight bytes `CPBEADS1`, a `u32` count,
then per bead `f32` `a.x a.y a.z b.x b.y b.z rx rz` and a `u32` material
index (36 bytes). The count must match the bytes.

The scene file:

```json
{
  "beads": "part.beads",
  "materials": [
    {"preset": "pla_matte", "color": "#4c8fd6", "roughness": 0.8, "subsurface": 0.25},
    {"preset": "pla_silk", "color": "#d4a64a", "metallic": 0.45, "anisotropy": 0.85}
  ],
  "boxes": [
    {"lo": [-6, -6, -1], "hi": [356, 326, 0],
     "material": {"color": "#4a4236", "metallic": 0.45, "roughness": 0.5, "texture": "stipple", "texture_scale": 0.22}}
  ],
  "floor_z": 0,
  "max_len": 3,
  "camera": {"eye": [40, -180, 120], "target": [175, 160, 10], "up": [0, 0, 1], "fov_degrees": 28},
  "render": {"width": 1600, "height": 1000, "samples": 96, "threads": 12,
             "denoise": true, "f_number": 0, "lighting": "product", "exposure": 1,
             "backdrop": "#d9d7d2", "tonemap": "agx", "adaptive": 0.004,
             "lights": [{"azimuth_degrees": -42, "elevation_degrees": 48, "intensity": 2.7, "radius_degrees": 26},
                        {"azimuth_degrees": 62, "elevation_degrees": 18, "intensity": 0.7, "radius_degrees": 34},
                        {"azimuth_degrees": 168, "elevation_degrees": 26, "intensity": 2.6, "radius_degrees": 12}]}
}
```

- `materials`: a preset (`docs/starlark-api.md`, materials) with any field
  overridden; `color` is sRGB `#rrggbb`. Bead index `n` uses material `n`.
- `subsurface`: the mean free path, in mm, of light under the surface of an
  opaque material (0 is off). The diffuse part of the surface becomes a random
  walk through the beads, so light that goes into a bead's edge or a thin wall
  comes out a little way off: soft edges, and thin walls that glow when lit
  from behind. The path given is a white material's; a darker one absorbs
  more on the way. A few tenths of a millimetre suits most plastics.
- `anisotropy` on a bead runs along it: smooth in the direction the nozzle
  moved, rough across it, which is the satin streak of a silk filament.
- `transmission` makes a see-through bead. Where see-through or scattering
  beads overlap, a crossing buried inside a neighbour is not a surface: the
  beads are one solid, and the gaps between beads that do not touch are real
  interfaces that scatter light, as they do in a printed part.
- `flakes` (beads only): small tilted mirrors in the plastic, for glitter,
  metal-filled and "galaxy" filaments and the faint glints of cut carbon
  fibre: `{"density", "size", "tilt_degrees", "roughness", "color"}`. Flakes
  sit on a lattice `size` mm apart; `density` is the share of lattice cells
  that hold one, a flake is a little over half a cell across, tilted from the
  surface by up to `tilt_degrees`, and reflects like a metal of `color`. A
  flake lights up only where it mirrors a light toward the camera, so below a
  pixel they are a sparkle the samples average, as glitter is.
- `speckle` (beads only): particles of another colour in the plastic, the
  grains of a stone-look filament or the fibres of a wood-filled one:
  `{"density", "size", "color"}` on the same kind of lattice.
- `streak` (beads only): how much the tone wanders from layer to layer, 0..1,
  in bands a layer or two tall that drift slowly along the layers.
- `boxes`: axis-aligned boxes with a material, to stand the beads on.
- `floor_z`: the plane the beads stand on; subsurface light that reaches the
  bottom of the first layer goes into it.
- `max_len`: beads longer than this (mm) are cut into pieces for the tree.
- `render.lighting`, `backdrop`, `floor`, `tonemap`: as `cadpole render`
  (`docs/views.md`); `tonemap` also takes `agx`, a log encoding and sigmoid
  that rolls saturated highlights off toward white and keeps the separation
  in deep shadows. `f_number` is a thin lens focused on the camera target.
- `render.lights`: the product rig's key, fill and rim, in that order, placed
  by hand: degrees around the subject from the camera (positive to the
  right), degrees up, irradiance, and angular radius. A black print shows its
  shape mostly by reflection: a large soft key high in front lays a broad
  highlight along its tops, and a rim behind draws its silhouette.
- `render.adaptive`: adaptive sampling. A pixel stops once the standard error
  of its mean, on a square-root scale of luminance, is below this (0.004, the
  default, is about one 8-bit step; 0 takes every sample everywhere). It
  never stops before sixteen samples or a quarter of `samples`, and the image
  is the same for any thread count.

## Cost

Beads are kept in a bounding volume hierarchy built with a binned surface
area heuristic, as a binary tree whose top levels are split one after
another and whose subtrees below 16 384 beads are built in parallel (the
tree is the same for any thread count), then collapsed into a four-wide tree
whose nodes test four child boxes at once: 36 bytes a bead and 128 a node,
about 60 bytes a bead in all. A million and a half beads build in under half
a second on a desktop. Each child box records the kinds of bead under it
(opaque, see-through, scattering), so a shadow ray looks only at the opaque
beads and then only at the see-through ones, and never descends into a
subtree with neither.

Opaque beads cost about what triangles do. Scattering beads cost a random
walk where the camera sees them; light that has already bounced sees them as
the Lambert surface the walk averages to. See-through beads cost several
times more per path, because a ray inside them crosses many bead walls.

## The `stipple` texture

A fine random grain with relief, for a powder-coated or textured sheet
(`texture="stipple"`, `texture_scale` the grain size in mm). Up close it
tilts the normal and speckles the colour and roughness, over clouds a few
grains across; once a grain is smaller than the filter it is the evenly
clouded, slightly rougher surface the grains average to. Both renderers draw
its colour; the path tracer also draws its relief, and filters the grain
over one pixel rather than three, because a random grain has no period to
beat against the pixel grid: grains a little smaller than a pixel still
glint, as a textured sheet does at arm's length.
