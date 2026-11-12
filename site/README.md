# The Cadpole site

One static page with live 3D, built from this repository: the scripts in
`examples/` and `site/features/`, the web app in `src/web`, and the CLI.

The page is in three parts, each opened by a heading of its own
(`groups` in `content/buildings.json`), after an opening that names the two
kinds of work side by side:

1. **Parts and assemblies**: the scroll scenes and the playground.
2. **Buildings and plans**: the floor plan, the lot, the shadow study, the
   solved rooms and the drawing set. Lengths read in feet and inches, and
   every building, lot and rule is invented.
3. **What both share**: the modelling operations, the vector drawings, the
   outputs, the tools and the agent recording.

## Layout

| Path | What it is |
| --- | --- |
| `build/` | The generator: `build.mjs` writes `index.html` and assembles the directory; `models.mjs` is what the page shows; `slim.mjs` packs a published model for the page's own 3D; `linkify.mjs` links the repository's address and the repository paths the text names (`docs/*.md`, `examples/*.star`, `./bin/install`) to GitHub, in prose and in the boxes to copy, and stops the build on a path the repository does not have |
| `src/` | `site.css`, `site.js`, and the engine's worker wrapper |
| `views.bzl`, `site.bzl` | The viewer pages, one `cadpole publish` per model, and the gallery's print files, one `cadpole plate` per model (`PRINTS`): a card whose model has one gets a "Print it" download of `print/<name>.3mf`. A card's "Script" link opens the script on GitHub, since a static host serves a `.star` file as a download |
| `features/` | The script behind each gallery tile; the lines between `# snippet` and `# end snippet` are the ones its card shows. `content/features.json` and `content/drawings.json` give each card its `kind`: a `model` opens in 3D, a `drawing` opens its SVG |
| `content/` | The cards' and captions' words; `layout.json` is the solver's answer, written by `render -- buildings` |
| `static/` | Pictures and drawings that are in the tree: stills, vector drawings, icons, the social card, each scene's first frame |
| `media.lock.json` | The large media that is not: frame sets and the screen recording, by count and digest |
| `rendering/` | How every picture is rendered |
| `checks/` | The page rules, as a Playwright suite |
| `tools/` | `serve.mjs`, `assemble.mjs` |

The page's 3D module is `src/web/src/site/site3d.ts`. It lives in the web
package because it is built from that package's modules (the engine worker,
the camera controller, the studio lighting) and is type-checked with them.

## Build and look

```
bazel build //site:dist          # bazel-bin/site/dist
bazel run //site:serve           # http://127.0.0.1:8766/
bazel run //site:serve -- --port 8800 --root /path/to/an/assembled/site
```

`//site:dist` is the whole site except the scroll scenes' frames and the
recording: each scene shows its first frame, and everything else works,
including "Explore in 3D", the playground and the viewer pages. The build runs
`cadpole publish` for each of the models in `views.bzl` with the optimized CLI.

## What is in the tree and what is not

In `static/`, about 4 MB in 131 files: the stills and the building sections'
raster pictures (1.2 MB), the vector drawings as SVG, PDF and DXF and the one
picture drawn under them (2.0 MB), the scenes' first frames (0.3 MB), the
social card, the icons and the recording's poster frames.

Not in the tree, 44 MB: the four scenes' frame sets (846 pictures in AVIF and
WebP) and the recording (four video files). They take hours of path tracing
to make and change rarely, so the tree holds their recipe and
`media.lock.json` holds their count and digest per directory.

## Assemble for publishing

```
bazel run //site:assemble -- --media /path/to/media --out /path/to/gh-pages
```

`--media` is any directory with `seq/` and `agent/` laid out as the site has
them: the output of `render -- encode` plus the recording, or a copy of the
site as last published. Every file is checked against `media.lock.json` first.
`--out` is replaced (a `.git` directory and a `CNAME` file in it are kept), and
is then the complete site, `.nojekyll` included: commit it to the branch the
host serves.

## Re-render

Needs `python3` and `ffmpeg` (with libaom, libwebp and librsvg).

```
bazel run --config=opt //site:render -- help
bazel run --config=opt //site:render -- stills feat-loft mascot   # into static/media
bazel run --config=opt //site:render -- drawings                  # into static/draw (`drawings cabin`: the plan sheets only)
bazel run --config=opt //site:render -- buildings                 # the building sections: seconds, no path tracing
bazel run --config=opt //site:render -- frames iris               # masters under --work
bazel run --config=opt //site:render -- jet                       # 96 renders; restartable
bazel run --config=opt //site:render -- encode                    # masters -> <work>/media/seq
bazel run --config=opt //site:render -- lock                      # media.lock.json
```

The headline, the sub-line and the link preview's text are
`content/site.json`; after changing them, `python3
site/rendering/social_card.py` redraws `static/og.png`.

`sets.py` names every picture, its script and its camera. After `encode`,
copy the recording into the media directory (`agent/*.mp4`, `agent/*.webm`;
it is a screen recording, not a render), run `lock`, and commit
`media.lock.json` with the first frames `encode` put under `static/seq/`.

`frames` and `jet` draw a faint ground grid under the model, in each frame's
own perspective, from the camera file the renderer writes beside the frame.
The plane is the lowest the model reaches in the whole sequence, the lines
sit on a lattice fixed in the world, and the grid has no outline: it fades
with distance from the model's footprint, toward the horizon and toward the
edge of the frame. `rendering/ground_grid.py` holds the numbers (`GRID`);
`--no-ground-grid` leaves it out. The stills have no grid.

Framing and encoding work from the masters under `--work`, so changing the
grid or the framing does not render anything again: run `frames` or `jet`,
then `encode`, then `lock`.

The registration of the drawings section's picture and its line drawing
(`frame` in `sets.py`) is measured, not computed: if the thumbscrew or its
camera changes, the square of the sheet the picture covers has to be measured
again. The same goes for the pillow block's hidden-line card. `drawings NAME`
redraws only the named drawings.

## Checks

```
bazel test //site:checks_test
bazel test //site:checks_test --test_env=CADPOLE_SITE_ROOT=/path/to/an/assembled/site
```

On five window sizes: no button or link over a scene's model, the model
centred and whole; no page zoom, no sideways scroll, 44 px tap targets, 16 px
form text; a reload lands where the visitor was. Once: every local link has a
file, viewer links stay in the site, outside links go to the repository, the
words the page does not use; and the live 3D (a lightbox, a scene, the
playground's engine, a viewer page) on a phone-sized window, and on a
desktop one that whoever holds the page's one 3D stage has drawn a model
(pixels read back from the canvas) through a window resize, that the
playground gets the stage back after the lightbox borrowed it, and that a
lost WebGL context is replaced. The first form
runs in the normal test sweep against `//site:dist`; the second runs the same
rules over every frame of an assembled site.
