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
`bead_mb`, `build_ms`, `trace_s`, ...). The command reads the two files it is
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
             "denoise": true, "f_number": 0, "lighting": "product", "exposure": 1}
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
- `boxes`: axis-aligned boxes with a material, to stand the beads on.
- `floor_z`: the plane the beads stand on; subsurface light that reaches the
  bottom of the first layer goes into it.
- `max_len`: beads longer than this (mm) are cut into pieces for the tree.
- `render.lighting`, `backdrop`, `floor`: as `cadpole render` (`docs/views.md`).
  `f_number` is a thin lens focused on the camera target.

## Cost

Beads are kept in a binary bounding volume hierarchy built with a binned
surface area heuristic: 36 bytes a bead and 32 a node, about 55 bytes a bead
in all. A million and a half beads build in about a second on one core. Opaque beads
cost about what triangles do; see-through beads cost several times more per
path, because a ray inside them crosses many bead walls.

## The `stipple` texture

A fine random grain with relief, for a powder-coated or textured sheet
(`texture="stipple"`, `texture_scale` the grain size in mm). Up close it
tilts the normal and speckles the colour and roughness; once a grain is
smaller than a pixel it is the even, slightly rougher surface the grains
average to. Both renderers draw its colour; the path tracer also draws its
relief.
