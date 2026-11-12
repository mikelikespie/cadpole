// What the page shows: the pinned scenes, the playground's scripts, the cards
// and the gallery. Viewer pages and packed models are named here by the names
// views.bzl publishes them under.

// A scene's step: [from, to, camera "az,el,fov,zoom", model for the live view
// or null for the scene's own, heading, text, alt text of its picture].
// `read` is the readout beside the progress bar, piecewise linear in scroll
// progress: the parameter the frames were rendered along (rendering/sets.py).
export const SCENES = [
  {
    id: "turbojet", seq: "tj", ar: "16 / 9", arv: 1.7778, arp: "4 / 3", len: 5, eyebrow: "One script, a whole assembly", model: "models/turbojet_cut.cpmesh",
    read: { label: "explode", keys: [[0, 0.0], [0.06, 0.0], [0.15, 0.022], [0.25, 0.088], [0.35, 0.187], [0.49, 0.354], [0.63, 0.521], [0.73, 0.62], [0.83, 0.687], [0.92, 0.709], [1, 0.709]], digits: 2, unit: " in" },
    steps: [
      [0, 0.07, "-32,16,24,0.67", null, "HERO", "", "A turbojet engine model with its casings cut away"],
      [0.07, 0.26, "-32,16,24,0.67", null, "A whole engine, one model.", "Fan, three compressor stages, combustor, turbine and nozzle: 108 parts in one nested assembly from <code>turbojet.star</code>. Blade counts, radii and clearances are parameters an agent, or you, can change.", "The assembled turbojet model, casings cut away"],
      [0.26, 0.48, "-29,17,24,0.64", null, "Cut open by a flag.", "<code>cutaway = True</code> takes a wedge out of every casing, frame and vane ring, so the rotor, the liner and the fuel injectors show. <code>cutaway_angle</code> sets how far it opens.", "The turbojet with a wedge cut from every casing"],
      [0.48, 0.78, "-22,18,24,0.6", "models/turbojet_open.cpmesh", "Pull it apart.", "<code>explode</code> is a parameter like any other. Every stage slides out along the shaft, and the bolts go with their flanges.", "The turbojet with its stages spreading along the shaft"],
      [0.78, 1.01, "-18,19,24,0.58", "models/turbojet_open.cpmesh", "It checks itself.", "Fan, compressor, combustor, turbine, nozzle. The model's own tests measure the tip clearance of every blade, so whoever changes it knows at once whether it still fits.", "The turbojet fully exploded, casings cut away"],
    ],
  },
  {
    id: "iris", seq: "ir", ar: "1", arv: 1, len: 3.5, eyebrow: "Mechanisms that move", model: "models/iris.cpmesh",
    read: { label: "opening", keys: [[0, 0], [1, 100]], digits: 0, unit: "%" },
    steps: [
      [0, 0.3, "-24,58,26,1", null, "An iris that prints in place.", "Eight blades on pivots under a slotted ring. Blade count, aperture and every clearance are parameters of <code>iris.star</code>.", "A closed eight-blade iris mechanism"],
      [0.3, 0.7, "-24,58,26,1", null, "Turn the ring.", "<code>opening</code> runs from 0 to 1. Each drive pin rides a spiral cam slot cut along its own path, so the ring turns 36 degrees from shut to open and the picture is the mechanism.", "The iris half open"],
      [0.7, 1.01, "-24,58,26,1", null, "Fully open.", "The blades sit behind the lip. The gaps are whole layers, so it comes off the printer already moving.", "The iris fully open"],
    ],
  },
  {
    id: "mug", seq: "mg", ar: "1", arv: 1, len: 4, eyebrow: "Artwork to geometry", model: "models/mug.cpmesh",
    read: { label: "relief", keys: [[0, 0.016], [0.333, 0.016], [0.722, 0.063], [0.736, 0.063], [1, 0.039]], digits: 3, unit: " in" },
    steps: [
      [0, 0.34, "-58,14,24,1", null, "Wrap an SVG round it.", "<code>import_svg</code> reads the drawing. <code>wrap</code> lays it round the wall of the mug without stretching it.", "A mug with flat artwork painted on its wall"],
      [0.34, 0.73, "-30,16,24,1", null, "Raise it.", "<code>emboss(kind=emboss.raised)</code> turns the outline into solid relief. How far it stands out is a parameter.", "The same mug with the artwork raised from the wall"],
      [0.73, 1.01, "-4,20,24,1", null, "Or cut it in.", "Change one argument and the same artwork is engraved instead. Either way it exports as a printable solid.", "The same mug with the artwork cut into the wall"],
    ],
  },
  {
    id: "radial", seq: "ra", ar: "1", arv: 1, len: 4.5, eyebrow: "Explode, section, animate", model: "models/radial.cpmesh",
    read: { label: "crank", keys: [[0, 0], [0.5, 180], [0.6, 200], [1, 360]], digits: 0, unit: "°" },
    steps: [
      [0, 0.15, "-35,16,24,1", null, "Seven cylinders, one crank.", "A radial engine with a master rod and six link rods. Turn <code>crank</code> and every piston goes where the geometry puts it.", "A seven-cylinder radial engine model"],
      [0.15, 0.58, "-50,20,24,1", "models/radial_open.cpmesh", "Take it apart.", "Rods, pistons, barrels and heads move out along their own cylinders in layers, the propeller and nose case slide forward, and the crankshaft slides back. The keyframes live in the script, next to the parts.", "The radial engine exploded along its cylinders"],
      [0.58, 1.01, "-25,15,24,1", "models/radial_open.cpmesh", "See inside.", "The cutaway removes everything in front of the crank: the propeller, the nose case and the front half of the crankcase, barrels and heads. The master rod, its six link rods, the pistons and the valves show while it turns.", "The radial engine with the front half cut away"],
    ],
  },
];

// The playground's tabs: [key (its viewer page and poster), script in examples/, label, camera].
export const PLAY = [
  ["part", "part.star", "Mounting plate", "-32,36,24,1.15"],
  ["planter", "planter.star", "Planter", "-30,24,24,1"],
  ["bracket", "bracket.star", "Pivot bracket", "-35,30,24,1"],
  ["knob", "knurled_knob.star", "Knurled knob", "-30,26,24,1"],
];

// "What comes out".
export const CARDS = [
  { img: "gridfinity", alt: "A tray of storage bins in three colours", title: "Files for the printer", body: "One <code>export()</code> line per file. This set writes an STL per bin and a Bambu Studio 3MF: a plate per filament, every bin a named object.", tags: ["STL", "3MF"], model: "gridfinity", view: "-35,35,24,1", name: "Gridfinity set" },
  { img: "media/bracket_sheet.svg", alt: "A dimensioned A4 drawing of a pivot bracket with a section through its bore", title: "Dimensioned drawings", body: "Views, sections and dimensions drawn from the model as vectors, ready to print or send to a laser.", tags: ["SVG", "PDF", "DXF"], paper: true },
  { img: "imports", alt: "An imported mesh, an imported STEP part and an imported DXF gear", title: "Bring your files", body: "Read a mesh, a STEP part or a DXF profile, then keep working on it with the same operations: a sculpted owl fused to a drilled base, an exact pillow block filleted and fitted, an involute gear hubbed and keyed.", tags: ["STL", "STEP", "DXF"], model: "imports", view: "-35,30,24,1", name: "Imported files" },
  { img: "wheel", alt: "A path-traced compressor wheel", title: "Pictures and motion", body: "Every image on this page came from <code>cadpole render</code> and <code>cadpole animate</code>. <code>sweep</code> draws a sheet of variants.", tags: ["PNG", "GIF"], model: "wheel", view: "-30,30,24,1", name: "Compressor wheel" },
];

// The gallery: [key, title, text, camera, script in examples/].
export const GALLERY = [
  ["mascot", "Desk mascot", "The logo stood up: its SVG imported as a prism, the front edge rounded by a signed-distance blend, the colours inlaid as parts that touch, on a plinth with raised lettering.", "-30,24,24,1", "desk_mascot.star"],
  ["gridfinity", "Gridfinity set", "One script for the baseplate and every bin: sizes, dividers, scoops, label tabs and a stacking lip, plus a 3MF with a plate per filament.", "-35,35,24,1", "gridfinity_showcase.star"],
  ["enclosure", "Enclosure for a Raspberry Pi 5", "A printable snap-fit case around a model of the board, made from its published drawing. One table places each port and cuts its opening.", "35,28,24,1", "enclosure.star"],
  ["planetary", "Herringbone planetary", "Sun, planets and a chevron-fluted ring that print as one piece, already assembled, and turn 4:1 when they come off the bed.", "-30,38,24,1", "planetary.star"],
  ["wheel", "Compressor wheel", "Blades lofted between two curves in space, with a splitter in every passage.", "-30,30,24,1", "compressor_wheel.star"],
  ["thumbscrew", "Thumbscrew and nut", "Real ISO metric threads swept along a helix, and a straight knurl that runs out into turned chamfers. The nut threads on with real running clearance.", "-30,20,24,1", "thumbscrew.star"],
  ["plan-cabin", "Cabin", "An invented two-bedroom cabin, furnished. Rooms are rectangles in feet; walls, doors, furniture and deck follow, cut at 4 ft like a plan.", "-35,55,24,1", "cabin_plan.star"],
];
