// Example scripts for the gallery (static mode) and the mock session.
//
// These are written against the *real* API (docs/starlark-api.md) and have to
// stay that way: in session mode `loadExample` writes the chosen one into the
// document as main.star, where the real engine evaluates it. They were once
// written in a dialect only the mock accepted (`param(description=)`,
// `sketch(plane="XY")`, `circle(d=)`), and loading any of them into a `cadpole
// serve` session replaced a working script with one that errored on its first
// line. `//src/web:examples_test` now runs every entry here through
// `cadpole eval` and fails on the first diagnostic.
//
// The mock engine is not an interpreter: it scrapes `param("name", default,
// ...)` (with `doc=`), and the positions of `sketch(` and `extrude(`, from the
// text -- so each script keeps one sketch and one extrude per line, in the
// order base then boss.

export interface Example {
  id: string;
  name: string;
  description: string;
  source: string;
}

export const EXAMPLES: Example[] = [
  {
    id: "plate-boss",
    name: "Plate with boss",
    description: "A rectangular plate with a cylindrical boss on top. The default model.",
    source: `# Plate with a cylindrical boss.
# Parameters show up in the Parameters panel; drag them and the model re-evaluates.

width = param("width", 60, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 40, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 20, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 12, min=0, max=100, step=0.5, unit="mm", doc="Boss height (0 leaves the boss out)")
with_boss = param("with_boss", True, doc="Include the boss")
finish = param("finish", "matte", choices=["matte", "satin", "gloss"], doc="Surface finish: how rough the plate renders")

base_profile = sketch(XY).rect(width, depth)
base = extrude(base_profile, height, name="base")
base.material(roughness={"matte": 0.9, "satin": 0.5, "gloss": 0.15}[finish])

# The boss stands on the plate's top face, so it follows the plate when height changes.
if with_boss and boss_h > 0:
    boss_profile = sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2)
    boss = extrude(boss_profile, boss_h, name="boss")
`,
  },
  {
    id: "tall-post",
    name: "Tall post",
    description: "A small square base with a tall, thin post.",
    source: `# Tall post: a small footprint and a long boss.

width = param("width", 30, min=20, max=200, step=1, unit="mm", doc="Base width (X)")
depth = param("depth", 30, min=20, max=200, step=1, unit="mm", doc="Base depth (Y)")
height = param("height", 6, min=2, max=100, step=0.5, unit="mm", doc="Base thickness (Z)")
boss_d = param("boss_d", 10, min=2, max=120, step=0.5, unit="mm", doc="Post diameter")
boss_h = param("boss_h", 80, min=0, max=100, step=0.5, unit="mm", doc="Post height (0 leaves the post out)")

base = extrude(sketch(XY).rect(width, depth), height, name="base")
if boss_h > 0:
    post = extrude(sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2), boss_h, name="post")
`,
  },
  {
    id: "wide-plate",
    name: "Wide plate",
    description: "A wide, flat plate with no boss (boss_h = 0 leaves the second extrude out).",
    source: `# Wide plate. boss_h is zero, so the boss is left out.

width = param("width", 160, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 90, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 4, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 0, min=0, max=100, step=0.5, unit="mm", doc="Boss height (0 leaves the boss out)")

base = extrude(sketch(XY).rect(width, depth), height, name="base")
if boss_h > 0:
    boss = extrude(sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2), boss_h, name="boss")
`,
  },
  {
    id: "diagnostics",
    name: "Diagnostics demo",
    description: "A script with an oversized boss and an explicit fail(), to show errors and warnings.",
    source: `# Diagnostics demo: the boss is wider than the plate, and fail() is called.

width = param("width", 40, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 30, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 10, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 50, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 8, min=0, max=100, step=0.5, unit="mm", doc="Boss height")

base = extrude(sketch(XY).rect(width, depth), height, name="base")
if boss_d > min(width, depth):
    warn("boss diameter %s does not fit on the %s x %s plate" % (boss_d, width, depth))
if boss_h > 0:
    boss = extrude(sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2), boss_h, name="boss")

fail("this line is here to demonstrate error diagnostics")
`,
  },
  {
    id: "plates",
    name: "Print plates",
    description: "The plate and its boss exported as a Bambu Studio project on two plates, to show the plates view.",
    source: `# Print plates: a Bambu Studio project. Each export() call to the
# same .3mf path is one plate; the toolbar's Plates button shows where the
# export would put the bodies, and which filament each prints in.

width = param("width", 60, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 40, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 8, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 12, min=0, max=100, step=0.5, unit="mm", doc="Boss height (0 leaves the boss out)")
printer = param("printer", "bambu_a1", choices=["bambu_a1", "bambu_a1_mini", "bambu_p1s", "bambu_x1c", "prusa_mk4", "ender3"], doc="The machine whose bed the plates are laid out for")

base = extrude(sketch(XY).rect(width, depth), height, name="base")
base.material(color="#e8b84a")
if boss_h > 0:
    boss = extrude(sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2), boss_h, name="boss")
    boss.material(color="#2f8f8f")
    export(boss, "plates_demo.3mf", flavor=flavor.bambu, plate="Boss", printer=printer)
export(base, "plates_demo.3mf", flavor=flavor.bambu, plate="Base", extruder=3, printer=printer)
`,
  },
  {
    id: "layers",
    name: "Layers demo",
    description: "The plate and its boss on two display layers, to show the Layers panel: switch a layer off or ghost it without re-evaluating.",
    source: `# Display layers. layer(...) declares one; layer= on a builtin puts
# the body on it. The Layers panel switches a layer off and ghosts it
# without re-evaluating the script, and \`cadpole render --layer envelope=off\`
# does the same for an image.

width = param("width", 60, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
depth = param("depth", 40, min=20, max=200, step=1, unit="mm", doc="Plate depth (Y)")
height = param("height", 8, min=2, max=100, step=0.5, unit="mm", doc="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", doc="Boss diameter")
boss_h = param("boss_h", 12, min=0, max=100, step=0.5, unit="mm", doc="Boss height (0 leaves the boss out)")

part = layer("part", doc="The plate itself")
envelope = layer("envelope", doc="The boss, as an envelope to check against", opacity=0.5, color="#ff8800")

base = extrude(sketch(XY).rect(width, depth), height, name="base", layer=part)
if boss_h > 0:
    boss = extrude(sketch(on=base.faces(normal=(0, 0, 1)).one()).circle(boss_d / 2), boss_h, name="boss", layer=envelope)
`,
  },
];

export function defaultExample(): Example {
  return EXAMPLES[0] as Example;
}
