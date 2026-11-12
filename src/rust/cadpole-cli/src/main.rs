//! `cadpole`: evaluate, render, sweep, export and serve Cadpole documents.
//!
//! Every verb below is sugar over one operation of `cadpole.v1.CadpoleOps`
//! (`proto/cadpole/v1/ops.proto`), dispatched through `cadpole-ops`; `cadpole op` calls
//! the same operations by name with generated flags. The ergonomic verbs exist
//! only to shape input and format output the way a person wants it.
//!
//! Conventions (see docs/design.md): results on stdout, diagnostics on
//! stderr, `--json` for machine-readable output, exit 0 = ok, 1 = the script
//! or operation failed, 2 = bad usage/input.

mod connect;
mod mcp;
mod opt_jobs;
mod runtime;
mod security;
mod server;
#[cfg(all(test, unix))]
mod transport_e2e_tests;

/// `println!` for the commands' output, which ends the process quietly, with
/// exit 0, when stdout is a pipe whose reader has gone (`cadpole doc | head`):
/// the reader has what it asked for. Rust ignores SIGPIPE, so std's
/// `println!` sees EPIPE and panics instead (exit 101). Any other write
/// error still panics, as std's does. Defined before the modules that print
/// command output so that they get it too.
macro_rules! println {
    () => {
        $crate::print_out(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::print_out(format_args!("{}\n", format_args!($($arg)*)))
    };
}

/// `print!`, likewise.
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::print_out(format_args!($($arg)*))
    };
}

mod input;
mod op_cli;
mod ops_host;
mod publish;
mod reply_stream;

/// Set once stdout's reader has gone away (`| head -1`, `| grep -q`).
static STDOUT_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where [`println!`] and [`print!`] write.
///
/// A closed pipe stops the writing, not the command: it still finishes its
/// file writes and returns its own exit code (a failing `cadpole test`
/// piped into `head` still exits 1). Exiting 0 on the spot threw that away.
fn print_out(args: std::fmt::Arguments<'_>) {
    use std::io::Write as _;
    use std::sync::atomic::Ordering;
    if STDOUT_CLOSED.load(Ordering::Relaxed) {
        return;
    }
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_fmt(args).and_then(|()| out.flush()) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            STDOUT_CLOSED.store(true, Ordering::Relaxed);
            return;
        }
        panic!("failed printing to stdout: {e}");
    }
}

/// True once stdout's reader has gone away.
fn stdout_closed() -> bool {
    STDOUT_CLOSED.load(std::sync::atomic::Ordering::Relaxed)
}

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Args, Parser, Subcommand};
use cadpole_engine::cadpole_core::ParamValue;
use cadpole_engine::cadpole_core::render::{EdgeMode, GridMode, Image, RenderOptions, contact_sheet, contact_sheet_clear, contact_sheet_with};
use cadpole_engine::cadpole_core::sun::{self as solar, SunSpec};
use cadpole_engine::cadpole_proto::prost;
use cadpole_engine::cadpole_proto::prost::Message as _;
use cadpole_engine::cadpole_proto::v1 as pb;
use cadpole_ops::Host;

/// Where a new user starts in each kind of work: the building commands and
/// units are spread over `render`, `export`, `solve` and `publish`, and
/// nothing in the command list says so.
const AFTER_HELP: &str = "\
Two kinds of work, one engine:
  Parts and assemblies (millimetres; docs/parts.md)
    cadpole render examples/bracket.star -o bracket.png
    cadpole check examples/print_ready.star
  Buildings and plans (feet and inches; docs/buildings.md)
    cadpole export examples/house_set.star --run-exports --out-dir .    a drawing set
    cadpole render examples/shadow_study.star --shadow-study --sun LAT,LON,DATE,TIME,TZ -o study.png
    cadpole solve examples/plan_parti_compare.star -p refit=cross_stair    a layout optimisation
  A building script declares units(units.ft_in); one drawn in plain feet
  takes --model-unit ft on export, bake and publish.";

#[derive(Parser)]
#[command(
    name = "cadpole",
    version,
    about = "Parametric CAD for bots: parts and assemblies, buildings and plans",
    after_help = AFTER_HELP
)]
struct Cli {
    /// The largest asset, in MB, read from `./assets` beside a script.
    /// By default a mesh (STL, OBJ, 3MF, glTF and a glTF's `.bin`)
    /// may be 512 MB and anything else 32 MB; this sets one cap for every
    /// kind, and four times it for the document. Uploads to `cadpole serve`
    /// and the browser's engine keep their 32 MB.
    #[arg(long, global = true, env = "CADPOLE_MAX_ASSET_MB", value_name = "MB")]
    max_asset_mb: Option<u64>,
    /// The Starlark heap one script may fill, in MB: the values it
    /// and each script it loads hold while they run, every file counted on
    /// its own. 512 MB by default, as in `cadpole serve`, the browser's engine
    /// and the apps; a script that loads a script that loads a model of a
    /// few thousand bodies can need more. Applies to every evaluation this
    /// process runs, `cadpole serve`'s included (a request may ask for less,
    /// never more); the browser and the apps keep 512 MB.
    #[arg(long, global = true, env = "CADPOLE_MAX_HEAP_MB", value_name = "MB")]
    max_heap_mb: Option<u64>,
    /// Wall-clock budget for one script evaluation, in seconds: 120 by
    /// default, so a script that never ends still stops and says so. Raise
    /// it for a model that is slow by design (a fine SDF mesh for a print);
    /// 0 means no limit. Applies to every command that evaluates a script
    /// except `cadpole serve`, which has its own `--eval-deadline-ms`.
    #[arg(long, global = true, env = "CADPOLE_EVAL_BUDGET_SECS", value_name = "SECS")]
    eval_budget_secs: Option<u64>,
    #[command(subcommand)]
    cmd: Cmd,
}

/// The Starlark heap every evaluation in this process may fill, in bytes:
/// set once by [`run`] from `--max-heap-mb` / `CADPOLE_MAX_HEAP_MB`,
/// [`cadpole_engine::Limits::DEFAULT_HEAP_BYTES`] otherwise. `cadpole serve`
/// installs it as its process default too.
static HEAP_BYTES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// See [`HEAP_BYTES`].
pub(crate) fn heap_bytes() -> usize {
    HEAP_BYTES.get().copied().unwrap_or(cadpole_engine::Limits::DEFAULT_HEAP_BYTES)
}

/// `--eval-budget-secs` as a deadline in milliseconds: the batch budget
/// when unset, 0 (no deadline) when asked for.
fn eval_deadline_ms_of(secs: Option<u64>) -> u64 {
    secs.map_or(cadpole_engine::Limits::batch().deadline_ms, |s| s.saturating_mul(1000))
}

/// `--max-heap-mb` in bytes: at least 1 MB, never "no cap".
fn heap_bytes_of(mb: u64) -> usize {
    usize::try_from(mb.max(1)).unwrap_or(usize::MAX).saturating_mul(1024 * 1024)
}

#[derive(Args, Clone)]
struct EvalArgs {
    /// A `.star` script (sibling `.star` files are loadable) or a packed document.
    input: PathBuf,
    /// Parameter override, `name=value`; repeatable.
    #[arg(long = "param", short = 'p', value_name = "NAME=VALUE")]
    params: Vec<String>,
    /// Tessellation tolerance in mm (default 0.05): how finely surfaces are
    /// triangulated for display and export -- the chordal sag no triangle
    /// exceeds. A spline patch is also divided until its normal turns by at
    /// most 8 degrees across a cell, so shading follows a twist.
    #[arg(long)]
    tolerance: Option<f64>,
    /// Boolean tolerance in mm (default 0.05). Separate from --tolerance
    /// because kernel cost grows sharply as it shrinks.
    #[arg(long)]
    boolean_tolerance: Option<f64>,
}

/// What `cadpole bake` and `cadpole publish` put in a bake besides the model.
#[derive(Args, Clone)]
struct BakeArgs {
    /// The name the viewer shows (default: the script's file name).
    #[arg(long)]
    title: Option<String>,
    /// Put the scripts in the bake: the entry and everything it loads,
    /// transitively (not every sibling), with its assets' names and hashes
    /// but never their bytes. Also keeps the script's `print()` output,
    /// which is otherwise left out. Off by default.
    #[arg(long)]
    include_sources: bool,
    /// Bake even when the script reports an error (the model as far as it
    /// got). By default an error stops the bake.
    #[arg(long)]
    allow_errors: bool,
    /// What one unit of the script is: mm (default), cm, m, in or ft. A
    /// script drawn in feet (`box(30, 20, 9)` for a 30 x 20-ft room) is
    /// scaled to millimetres once, here, so the viewer's readouts and grid
    /// are true; with ft or in the display defaults to feet and inches.
    #[arg(long, value_name = "UNIT")]
    model_unit: Option<String>,
    /// How the viewer writes a length: mm, cm, m, in, ft or ft-in. Default:
    /// the script's own `units(...)`, else ft-in for --model-unit ft or in.
    #[arg(long, value_name = "UNIT")]
    units: Option<String>,
    /// The view the page opens on, framed on the visible bodies: home
    /// (default), iso, top, bottom, front, back, left or right. Default:
    /// the script's `render_defaults(camera=)` if it declares one, else home.
    #[arg(long = "view", value_name = "PRESET")]
    open_view: Option<String>,
    /// A free opening camera: the eye at `x,y,z`, in the script's units
    /// (feet with --model-unit ft), looking at --target.
    #[arg(long, value_name = "X,Y,Z", conflicts_with = "open_view")]
    eye: Option<String>,
    /// What --eye looks at, `x,y,z` in the script's units (default: the
    /// centre of the visible bodies).
    #[arg(long, value_name = "X,Y,Z", requires = "eye")]
    target: Option<String>,
    /// The viewer's ground grid: on (default), dim or off. The viewer can
    /// still turn it back on (View > Grid).
    #[arg(long, value_name = "on|dim|off")]
    grid: Option<String>,
    /// A layer that starts hidden, by name; repeatable. Over the script's
    /// own defaults (a layer the script hides stays hidden); the viewer can
    /// still show it from the Layers panel.
    #[arg(long = "hide-layer", value_name = "NAME")]
    hide_layer: Vec<String>,
    /// Which parameters the page lists, read-only, with the values it was
    /// baked at: a comma list of names, `all`, or `none` (the default -- no
    /// Parameters panel). A published page never re-evaluates, so none of
    /// them is a slider.
    #[arg(long, value_name = "NAMES")]
    show_params: Option<String>,
    /// A section the page opens with, as `cadpole render --section` spells
    /// it, in the script's units: `z=4` removes everything above z = 4;
    /// `z=4,x=12` adds a vertical cut; `-x=12` removes the -x side. The
    /// viewer can move or clear it.
    #[arg(long, value_name = "SPEC")]
    section: Option<String>,
    /// A layer the camera frames, by name; repeatable. The opening view,
    /// Home, Iso and Fit then fit only these layers' drawn bodies -- the
    /// house, say, not the neighbours beside it, which stay drawn.
    #[arg(long = "frame-layer", value_name = "NAME")]
    frame_layer: Vec<String>,
    /// The display mode the page opens in: shaded, shaded-edges (the
    /// default), wireframe, xray (every body see-through, so what is inside
    /// a casing shows) or faces. The viewer can change it, and `?mode=` in
    /// the page's address overrides it.
    #[arg(long, value_name = "MODE")]
    mode: Option<String>,
    /// A link back to the site this page belongs to, drawn beside the
    /// page's title: an http(s) address, or one relative to the page
    /// (`../#models`).
    #[arg(long = "home-url", value_name = "URL")]
    home_url: Option<String>,
    /// What the --home-url link is called (default: Back).
    #[arg(long = "home-label", value_name = "TEXT", requires = "home_url")]
    home_label: Option<String>,
}

/// `x,y,z` in the script's units, as a point in millimetres.
fn publish_point(flag: &str, s: &str, scale: f64) -> anyhow::Result<pb::Vec3> {
    let v: Vec<f64> = s.split(',').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| anyhow::anyhow!("--{flag} {s:?}: expected x,y,z"))?;
    anyhow::ensure!(v.len() == 3 && v.iter().all(|x| x.is_finite()), "--{flag} {s:?}: expected three numbers, x,y,z");
    Ok(pb::Vec3 { x: v[0] * scale, y: v[1] * scale, z: v[2] * scale })
}

/// The `PublishedView` the flags ask for, or `None` for none.
fn published_view(args: &BakeArgs, scale: f64) -> anyhow::Result<Option<pb::PublishedView>> {
    let mut v = pb::PublishedView::default();
    if let Some(p) = &args.open_view {
        let p = p.trim().to_ascii_lowercase();
        anyhow::ensure!(publish::VIEW_PRESETS.contains(&p.as_str()), "--view {p:?}: one of {}", publish::VIEW_PRESETS.join(", "));
        v.camera = p;
    }
    if let Some(e) = &args.eye {
        v.eye = Some(publish_point("eye", e, scale)?);
    }
    if let Some(t) = &args.target {
        v.target = Some(publish_point("target", t, scale)?);
    }
    if let Some(g) = &args.grid {
        let g = g.trim().to_ascii_lowercase();
        anyhow::ensure!(matches!(g.as_str(), "on" | "dim" | "off"), "--grid {g:?}: on, dim or off");
        v.grid = g;
    }
    if let Some(s) = &args.section {
        v.section = publish::section_spec(s, scale)?;
    }
    v.frame_layers = args.frame_layer.clone();
    if let Some(m) = &args.mode {
        v.mode = publish::display_mode(m)?;
    }
    if let Some(u) = &args.home_url {
        v.home_url = publish::home_url(u)?;
        v.home_label = args.home_label.as_deref().unwrap_or_default().trim().to_owned();
    }
    Ok((v != pb::PublishedView::default()).then_some(v))
}

#[derive(Args, Clone)]
struct ViewArgs {
    /// Image size, WxH. For `sweep` this is the size of one tile; the default
    /// there is smaller, because a sheet holds many of them.
    #[arg(long)]
    size: Option<String>,
    /// Camera direction: iso, front, back, top, bottom, left, right,
    /// iso-bottom, iso-back, or an angle pair like `az=35,el=-25` (azimuth
    /// from the front toward the right, elevation above the horizon).
    /// Default: the document's `render_defaults(camera=)` if it declares
    /// one, else iso.
    #[arg(long, value_name = "PRESET")]
    view: Option<String>,
    /// A free camera: stand the eye exactly here, `x,y,z` in model mm (a
    /// neighbour's window, a point on the sidewalk), looking at --target.
    /// Nothing is fitted to the model; --fov sets the lens (default 35).
    #[arg(long, value_name = "X,Y,Z", conflicts_with = "view")]
    eye: Option<String>,
    /// What --eye looks at, `x,y,z` (default: the model's centre).
    #[arg(long, value_name = "X,Y,Z", requires = "eye")]
    target: Option<String>,
    /// The camera's up vector for --eye, `x,y,z` (default 0,0,1).
    #[arg(long, value_name = "X,Y,Z", requires = "eye")]
    up: Option<String>,
    /// The sun: `lat,lon,YYYY-MM-DD,HH:MM[,tz]` computes it for a place, a
    /// date and a local clock time (tz is the clock's offset from UTC in
    /// hours; default the longitude's standard zone, no daylight saving),
    /// or `az=<deg>,el=<deg>` places it by compass azimuth (0 north, 90
    /// east) and elevation. Adds a directional light with sharp shadows
    /// to the rig. Default: the document's `render_defaults(sun=)`, if any.
    #[arg(long, value_name = "SPEC")]
    sun: Option<String>,
    /// Where north is in model space, in degrees clockwise from +y seen
    /// from above (default: the document's `render_defaults(north=)`, else
    /// 0, meaning +y is north).
    #[arg(long, value_name = "DEG")]
    north: Option<f64>,
    /// The sun's colour: `auto` (the default) follows its elevation
    /// through the air, near white high up, orange near the horizon;
    /// `#rrggbb` or a colour temperature such as `3500K` sets it, keeping
    /// the brightness.
    #[arg(long, value_name = "auto|#RRGGBB|KELVIN")]
    sun_color: Option<String>,
    /// Multiplier on the sun's irradiance (default 1).
    #[arg(long, value_name = "X")]
    sun_intensity: Option<f64>,
    /// Path tracing: aerial perspective, `<distance>[unit][,#rrggbb]`: at
    /// that distance half of what the camera sees is haze (`400m`,
    /// `1200ft`; a bare number is mm). Coloured by the sky at the horizon
    /// unless a colour is given; camera rays only, so no shadow dims.
    #[arg(long, value_name = "DIST")]
    haze: Option<String>,
    /// A shift lens: slide the frame up the image plane by this fraction
    /// of its height (negative: down) without turning the camera.
    #[arg(long, value_name = "FRACTION", allow_negative_numbers = true)]
    shift: Option<f64>,
    /// Level the camera and shift the lens so the target stays centred,
    /// as an architectural photographer does: vertical edges stay
    /// vertical.
    #[arg(long)]
    level: bool,
    /// Cut every body by a plane for this image only, capped: `z=40`
    /// removes everything above z = 40 (`x=`, `y=` likewise); `-x=22`
    /// removes the -x side instead.
    #[arg(long, value_name = "AXIS=VALUE", allow_hyphen_values = true)]
    section: Option<String>,
    #[arg(long)]
    no_edges: bool,
    #[arg(long)]
    no_grid: bool,
    /// Static edges: off (shaded only), silhouette, creases (default), all.
    #[arg(long, value_name = "MODE")]
    edges: Option<String>,
    /// Minimum angle in degrees between two surfaces for their shared edge to
    /// be drawn (default 30). Tangent seams measure ~0 and are never drawn.
    #[arg(long, value_name = "DEG")]
    crease: Option<f32>,
    /// Feature-edge width in pixels (default 1.15).
    #[arg(long, value_name = "PX")]
    line_width: Option<f32>,
    /// Ground grid: off, finite, infinite (default).
    #[arg(long, value_name = "MODE")]
    grid: Option<String>,
    /// Draw every body translucent so interiors are visible.
    #[arg(long)]
    xray: bool,
    /// A display layer for this image, repeatable: `--layer
    /// envelope=off` hides a layer the script declared with `layer(...)`,
    /// `--layer terrain=0.4` ghosts one at 40 % opacity, `--layer
    /// neighbours=on` shows one declared hidden; `other` is the bodies on
    /// no layer. A layer not named keeps its own defaults.
    #[arg(long = "layer", value_name = "NAME=on|off|OPACITY")]
    layer: Vec<String>,
    /// Render one labelled tile per edge style into a contact sheet instead
    /// of a single image, to compare them side by side.
    #[arg(long)]
    compare_edges: bool,
    /// Anti-aliasing samples per pixel: 1, 4 or 8 (render default 4; sweep
    /// defaults to draft unless this is given).
    #[arg(long)]
    samples: Option<u32>,
    /// Worker threads (default: every hardware thread). Never changes the image.
    #[arg(long)]
    threads: Option<u32>,
    /// Renderer: `raster` (fast preview, the default), `gpu` (the wgpu
    /// viewport renderer, headless -- the same look, on whatever GPU this
    /// machine has, falling back to `raster` if it has none, and for what it
    /// cannot draw -- a sun, --debug, --analysis, --lighting, --backdrop --
    /// with a note) or `path` (path traced, for final images: soft shadows,
    /// colour bleeding, real glass).
    #[arg(long, default_value = "raster")]
    backend: String,
    /// Path tracing only: maximum scattering events per path (default 8).
    #[arg(long)]
    bounces: Option<u32>,
    /// Disable screen-space ambient occlusion.
    #[arg(long)]
    no_ao: bool,
    /// Disable the ground contact shadow.
    #[arg(long)]
    no_shadow: bool,
    /// Disable silhouette lines.
    #[arg(long)]
    no_silhouettes: bool,
    /// Fast preview: 1 sample, no ambient occlusion, no contact shadow.
    #[arg(long)]
    draft: bool,
    /// Exposure multiplier applied before the tonemap (default 1.0).
    #[arg(long)]
    exposure: Option<f32>,
    /// What the model does not cover: a colour as `#rrggbb` (default a dark
    /// grey), written exactly, so it can match a page or a slide; or
    /// `transparent`, which writes no background at all. The PNG then
    /// carries straight alpha taken from each pixel's own samples, so an
    /// edge is as smooth over any page colour as the renderer drew it, and
    /// glass is partly clear. Lighting and reflections still come from the
    /// rig and --backdrop. Not for a GIF, which has no partial alpha.
    #[arg(long, value_name = "#RRGGBB|transparent")]
    background: Option<String>,
    /// With `--background transparent`: keep the shadow the model casts on
    /// the ground, as black at the shadow's own alpha, so the picture sits
    /// on whatever it is laid over. Off by default: nothing but the model.
    #[arg(long)]
    shadow_catcher: bool,
    /// Render N x N sub-pixels per pixel and filter them down (1..4,
    /// default 1): for thin features and line work the sample count alone
    /// leaves rough, at N squared times the pixels. The path tracer
    /// divides --samples by N squared, so its cost stays about the same.
    #[arg(long, value_name = "N")]
    supersample: Option<u32>,
    /// Path tracing only: show the studio's environment gradient behind the
    /// model instead of the flat background, so a metal or a glass has
    /// something to reflect. The rasterizer ignores it.
    #[arg(long)]
    environment: bool,
    /// Light rig: `studio` (default; the engineering look, fixed in world
    /// space) or `product` (a neutral catalogue rig locked to the camera:
    /// key 40 degrees to camera-left and 40 up, a soft fill, a narrow rim;
    /// docs/views.md "The product shot").
    #[arg(long, value_name = "RIG")]
    lighting: Option<String>,
    /// Surround: `studio` (grey cyclorama), `white`, `black` or `#rrggbb`.
    /// Sets the environment the path tracer shows and reflects, its floor
    /// colour, and (unless --background is given) the rasterizer's
    /// background to match. Unset keeps the rig's own gradient and the
    /// dark floor.
    #[arg(long, value_name = "BACKDROP")]
    backdrop: Option<String>,
    /// Path tracing: the floor's finish, `matte` (default), `gloss` (a
    /// faint reflection of the part) or `none`.
    #[arg(long, value_name = "FINISH")]
    floor: Option<String>,
    /// Tone curve: `aces` (default), `neutral` (Khronos PBR Neutral:
    /// hue-preserving, exact for base colours; the product-shot choice) or
    /// `none` (a plain clamp).
    #[arg(long, value_name = "CURVE")]
    tonemap: Option<String>,
    /// Path tracing: thin-lens depth of field at this f-number, focused on
    /// the model (f/8 is gentle, f/4 a hero shot). Off when absent.
    #[arg(long, value_name = "F")]
    dof: Option<f64>,
    /// Bloom: highlights over display white spill a little into their
    /// neighbours. Off by default because the halo crosses the silhouette,
    /// so the backdrop next to a highlight is no longer bit-exact.
    #[arg(long)]
    bloom: bool,
    /// Vertical field of view in degrees (default: the view preset's: 35,
    /// or 24 for `product` and 18 for `hero`).
    #[arg(long, value_name = "DEG")]
    fov: Option<f64>,
    /// Path tracing: shade every crease edge with an edge break of this
    /// radius in mm, so edges catch the light as the bright lines a real
    /// machined part shows (0.25 is a typical deburr). No geometry changes.
    /// A material's own `edge_break=` overrides it for that body.
    #[arg(long, value_name = "MM", num_args = 0..=1, default_missing_value = "0.25")]
    edge_break: Option<f64>,
    /// Path tracing: an edge-aware denoise of the finished image (guided by
    /// the first hit's normal, albedo and depth). Removes the grain left on
    /// glossy metals at a few hundred samples; also softens fine texture.
    #[arg(long)]
    denoise: bool,
    /// Colour-map a measured field over the surface with a legend, instead of
    /// the bodies' materials: `thickness` (wall thickness against --min-wall)
    /// or `overhang` (angle from vertical against --max-overhang).
    #[arg(long, value_name = "FIELD")]
    analysis: Option<String>,
    /// Minimum wall for `--analysis thickness`, in mm (default 0.8: two
    /// perimeters of a 0.4 mm nozzle). Red below it.
    #[arg(long, value_name = "MM")]
    min_wall: Option<f64>,
    /// Support threshold for `--analysis overhang`, in degrees from vertical
    /// (default 45).
    #[arg(long, value_name = "DEG")]
    max_overhang: Option<f64>,
    /// Draw a debug view of the mesh instead of the part: `faces` (each
    /// topology face group in its own colour), `normals` (shading normal as
    /// RGB), `defects` (crossings red, slivers orange, folds magenta, counts
    /// on stderr), `seams` (creases black, seams the crease rule smoothed
    /// over blue) or `wire` (every triangle edge). Flat-lit, opaque, one
    /// sample, on the CPU rasterizer whatever --backend says.
    #[arg(long, value_name = "VIEW")]
    debug: Option<String>,
    /// Print per-stage render timings to stderr.
    #[arg(long)]
    stats: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Evaluate a document and print a summary (or the JSON model view).
    Eval {
        #[command(flatten)]
        eval: EvalArgs,
        /// Print the JSON model view (meshes omitted) instead of a summary.
        #[arg(long)]
        json: bool,
        /// Print the full cadpole.v1.Model protobuf to this file.
        #[arg(long, value_name = "FILE")]
        model_out: Option<PathBuf>,
        /// What to do with optimisation solves too long for the evaluation:
        /// `off` reports them pending; `wait` runs them as jobs, as
        /// `cadpole solve` does, and then evaluates the document with their
        /// answers.
        #[arg(long, value_name = "off|wait", default_value = "off")]
        jobs: String,
    },
    /// Render to a PNG with the built-in software rasterizer.
    Render {
        #[command(flatten)]
        eval: EvalArgs,
        #[command(flatten)]
        view: ViewArgs,
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        /// A shadow study: one contact sheet of the model under the
        /// sun at the June solstice, the March equinox and the December
        /// solstice (rows, --dates) at three times of day (columns, --times),
        /// for the place and year `--sun lat,lon,YYYY-MM-DD,HH:MM[,tz]`
        /// names; or at exactly the instants `--at` lists. --size is the
        /// size of one tile.
        #[arg(long, requires = "sun")]
        shadow_study: bool,
        /// The dates a shadow study's rows are drawn at (default: the June
        /// solstice, the March equinox and the December solstice of the
        /// year --sun names).
        #[arg(long, value_name = "YYYY-MM-DD,...", requires = "shadow_study", conflicts_with = "at")]
        dates: Option<String>,
        /// The times of day a shadow study's columns are drawn at, local
        /// clock (default 09:00,12:00,15:00).
        #[arg(long, value_name = "HH:MM,...", requires = "shadow_study", conflicts_with = "at")]
        times: Option<String>,
        /// One tile of a shadow study at an exact instant, repeatable: `--at
        /// 2026-12-21,12:00 --at 2026-03-20,09:00 --at 2026-03-20,15:00`
        /// draws those three and nothing else, in that order (the place and
        /// the zone are still --sun's). Up to four in a row, then a grid.
        #[arg(long, value_name = "YYYY-MM-DD,HH:MM", requires = "shadow_study", action = clap::ArgAction::Append)]
        at: Vec<String>,
        /// Also write the camera this image was drawn with, as JSON: its
        /// position and its view and projection matrices, the ground plane
        /// the model stands on, and the model's bounding box in the world
        /// and in pixels. What a page needs to draw its own ground or
        /// labels in register with a transparent render (docs/views.md).
        #[arg(long, value_name = "FILE", conflicts_with_all = ["shadow_study", "compare_edges"])]
        camera_json: Option<PathBuf>,
    },
    /// Render an animation the script declares with `animation(name,
    /// keyframes, ...)`: the document is evaluated once per frame with the
    /// keyframes' values as overrides, through one memo, every frame drawn
    /// with one camera fitted to everything the keyframes show, which a
    /// keyframe's `camera=view(az=, el=, fov=, zoom=, pan=)` turns, zooms
    /// and pans without losing the subject. Writes an APNG or a GIF, or one
    /// PNG per frame into a directory; `--list` says what the script
    /// declares.
    Animate {
        #[command(flatten)]
        eval: EvalArgs,
        #[command(flatten)]
        view: ViewArgs,
        /// Which animation, by the name given to `animation(...)`. Needed
        /// only when the script declares several.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Where to write: `x.apng`, `x.gif`, or a directory for `--format
        /// frames` (`frame_0000.png` ...). Required unless `--list`.
        #[arg(short, long, value_name = "FILE|DIR")]
        out: Option<PathBuf>,
        /// `apng` (default), `gif` or `frames`; otherwise read off `--out`'s
        /// extension (`.gif`, `.apng`/`.png`, none for a directory).
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
        /// Frame rate, over the animation's own `fps`.
        #[arg(long)]
        fps: Option<f64>,
        /// Render exactly this many frames over the whole animation instead
        /// of one per 1/fps: a quick look.
        #[arg(long)]
        frames: Option<u32>,
        /// Evaluate every frame as a drag preview: fuses a keyframe reaches
        /// are skipped and their operands shown. Exact for an
        /// animation that only poses instances.
        #[arg(long)]
        preview: bool,
        /// List the animations the script declares and render nothing.
        #[arg(long)]
        list: bool,
        /// Render the animation once per named size instead of once at
        /// --size, repeatable: `--variant landscape=1800x1200 --variant
        /// portrait=1080x1920`. Each is framed for its own aspect ratio, so
        /// the subject fills a tall frame as well as a wide one. Frames go
        /// to `OUT/NAME/frame_0000.png`, a file to `OUT`'s name with
        /// `_NAME` before the extension, and --camera-json likewise.
        #[arg(long, value_name = "NAME=WxH", conflicts_with = "list")]
        variant: Vec<String>,
        /// Also write every frame's camera as JSON: position, view and
        /// projection matrices, the ground plane, and the model's bounding
        /// box in the world and in pixels (docs/views.md).
        #[arg(long, value_name = "FILE", conflicts_with = "list")]
        camera_json: Option<PathBuf>,
    },
    /// Render a labelled contact sheet over a parameter grid.
    ///
    /// `--param w=30,40,50 --param h=10,20` renders every combination.
    /// Every tile is a raster render of the whole model, so the render flags
    /// a sweep cannot carry (--section, --analysis, --debug, a --backend
    /// other than raster, the path-tracing knobs, --compare-edges, --stats)
    /// are refused.
    Sweep {
        #[command(flatten)]
        eval: EvalArgs,
        #[command(flatten)]
        view: ViewArgs,
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        /// Tile size, WxH. Same thing as `--size`, which also works here;
        /// this name reads better on a contact sheet. Default 320x240.
        #[arg(long)]
        tile: Option<String>,
        /// Tiles per row (default: number of values of the first param).
        #[arg(long)]
        cols: Option<u32>,
    },
    /// Export visible bodies: a mesh (stl, obj, 3mf), a scene for viewers
    /// and DCC tools (glb, gltf), exact surfaces (step), or a dimensioned
    /// drawing (svg, pdf, dxf), picked from --format or the --out
    /// extension. Add --run-exports to run the script's own `export*(...)`
    /// calls too.
    ///
    /// Bodies on a display layer the script declares hidden
    /// (`layer(..., visible=False)`) are left out, as a viewer leaves them
    /// out; --include-hidden writes them. OBJ groups each body (`o`) by its
    /// layer (`g`); 3MF names each object's layer in its metadata.
    ///
    /// glTF is one node per body, named for it, under one node per
    /// layer, with the document's materials, in metres with +Y up, each
    /// node's `extras.cadpole` naming its body, layer and whether it is
    /// hidden. A script drawn in feet needs --model-unit ft, as `cadpole bake`
    /// does: `cadpole export house.star --model-unit ft -o house.glb`.
    ///
    /// With --view or --section the svg, pdf or dxf is one page of one view,
    /// cut the way `cadpole render --section` cuts: the colour look
    /// paints what the render shows, the script's `extrude(text(...))` labels
    /// and the page's words as real text in the embedded face:
    /// `cadpole export house.star --model-unit ft --view top --section z=5.5
    /// --title "L0" -o l0.pdf`.
    ///
    /// STEP carries the exact surfaces, which is what a CAD tool wants. It
    /// needs every body on the B-rep path, so a model whose booleans went to
    /// the mesh kernel is refused with a diagnostic saying which body and
    /// what to change.
    Export {
        #[command(flatten)]
        eval: EvalArgs,
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// stl, obj, 3mf, glb, gltf, step, svg, pdf or dxf. Defaults to the
        /// --out extension.
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
        /// Also write the bodies on hidden display layers (the neighbours,
        /// the existing building, a reference scan). A glTF marks each one
        /// hidden in its node's extras; an OBJ with a `# hidden layer` line.
        #[arg(long)]
        include_hidden: bool,
        /// Write only the bodies on this display layer, by the name
        /// `layer(...)` declared (`other` is the bodies on no layer);
        /// repeatable. A hidden layer named here is written. Adds to --body.
        /// With --view or --section it is a display layer of the drawing
        /// instead, as `cadpole render --layer` takes it: `NAME=off`,
        /// `NAME=on`.
        #[arg(long = "layer", value_name = "NAME")]
        layers: Vec<String>,
        /// Write only this body, by id (`feature#n`, as `cadpole eval` lists
        /// them) or by name; repeatable. Adds to --layer.
        #[arg(long = "body", value_name = "ID")]
        bodies: Vec<String>,
        /// glTF and --view/--section drawings: what one unit of the script
        /// is -- mm (default), cm, m, in or ft. A script drawn in feet is
        /// written at its true size in metres (glTF), or scaled to
        /// millimetres for a drawing so its scale and scale bar are true
        /// (--section, --crop and --label stay in the script's units);
        /// without this its feet read as millimetres.
        #[arg(long, value_name = "UNIT")]
        model_unit: Option<String>,
        /// glTF: the display units recorded in the file; a --view/--section
        /// drawing: how its scale and scale bar write lengths -- mm, cm, m,
        /// in, ft or ft-in. Default: the script's own `units(...)`, else
        /// ft-in for --model-unit ft or in, else mm.
        #[arg(long, value_name = "UNIT")]
        units: Option<String>,
        /// 3MF only: which slicer's conventions to add on top of the 3MF core
        /// specification, so a coloured model opens already coloured.
        /// `core` writes only the standard; `prusa` (default) adds
        /// PrusaSlicer's namespaced attributes and config parts, which stay
        /// inside the core schema; `bambu` adds Bambu Studio / OrcaSlicer's
        /// unprefixed `paint_color` attributes and config parts too.
        #[arg(long, default_value = "prusa", value_name = "NAME")]
        slicer: String,
        /// 3MF only: `objects` (default) writes one object per body, a plate
        /// of parts; `part` merges them into one multi-material object.
        #[arg(long, default_value = "objects", value_name = "MODE")]
        group: String,
        /// 3MF only: `standard` (default) writes a plain 3MF; `bambu` writes
        /// a Bambu Studio project -- object files, plates, a filament colour
        /// list and plate thumbnails -- that opens with its plates, names,
        /// positions and colours intact.
        #[arg(long, default_value = "standard", value_name = "NAME")]
        flavor: String,
        /// Bambu flavor only: the plate (1-based) the whole-document export
        /// lands on. A script's own `export(..., plate=)` calls name theirs.
        #[arg(long, default_value_t = 1, value_name = "N")]
        plate: u32,
        /// Bambu flavor only: the machine the plates are laid out for and
        /// the pieces fitted to (bambu_h2d, bambu_h2c, bambu_x1c,
        /// bambu_a1_mini, ...). Default: the X1 Carbon's 256 mm bed.
        #[arg(long, value_name = "NAME")]
        printer: Option<String>,
        /// Bambu flavor only: a custom bed, X,Y or X,Y,Z in mm, replacing
        /// the printer's for the plate grid and the fit check.
        #[arg(long, value_name = "MM")]
        bed: Option<String>,
        /// Bambu flavor only: a Bambu Studio project you saved for your
        /// printer -- a path to its .3mf, or the name of an asset under
        /// ./assets. The project carries its printer, process and filament
        /// settings, the model's filament slots mapped onto its filaments,
        /// and is laid out on its printer's bed. A script's own
        /// `export(..., template=)` calls name theirs.
        #[arg(long, value_name = "FILE")]
        template: Option<String>,
        /// Bambu flavor only: also write a standalone image of every plate
        /// -- its parts in their filament colours standing on the bed, from
        /// a three-quarter view -- as `<stem>_plate_<n>_<name>.png`, and a
        /// contact sheet of every plate as `<stem>_plates.png`, under DIR.
        /// Covers --out's project and, with --run-exports, every project
        /// the script writes; a script's own `plate_images(...)` declaration
        /// sets its project's size and background itself. Without this flag
        /// the declared images go under --out-dir.
        #[arg(long, value_name = "DIR")]
        plate_images: Option<PathBuf>,
        /// Plate image size, WxH (default 1600x1200).
        #[arg(long, value_name = "WxH")]
        plate_image_size: Option<String>,
        /// What surrounds the plate in a plate image: `transparent`
        /// (default), `studio`, `white`, `black` or `#rrggbb`.
        #[arg(long, value_name = "NAME")]
        plate_image_background: Option<String>,
        /// Plate images only: `raster` (default, milliseconds) or `path`
        /// (path traced, for a finished shot; seconds to minutes).
        #[arg(long, default_value = "raster", value_name = "NAME")]
        backend: String,
        /// Execute the `export()` calls in the script, writing under --out-dir:
        /// a mesh, a drawing, a sheet set's PDF. Without --out that is all:
        /// the whole document is not exported beside them, so no body is
        /// meshed or checked for a file nobody asked for.
        #[arg(long)]
        run_exports: bool,
        #[arg(long, default_value = ".")]
        out_dir: PathBuf,
        /// A drawing of one view (svg, pdf or dxf), as `cadpole render`
        /// would frame it: `top` (a plan, the default with --section z=),
        /// or a direction to look from (`front`, `north`, `east`, ...).
        /// With --view or --section the drawing is the colour look -- every
        /// surface filled in its body's colour, the cut filled, the script's
        /// `extrude(text(...))` labels and the page's words as real text --
        /// on one page with a title, a scale bar and, on a plan, a north
        /// arrow.
        #[arg(long, value_name = "VIEW")]
        view: Option<String>,
        /// The cut, as `cadpole render --section` takes it: `z=1700` removes
        /// everything above z = 1700 and looks down (a floor plan); `x=26`
        /// removes everything beyond x = 26 and looks toward -x; `-x=26`
        /// keeps the other side; `y=` likewise. Any written length works.
        #[arg(long, value_name = "AXIS=VALUE", allow_hyphen_values = true)]
        section: Option<String>,
        /// `color` (the default with --view/--section) or `lines`, the
        /// drafting look: cut faces hatched, hidden-line-removed strokes.
        #[arg(long, value_name = "LOOK")]
        look: Option<String>,
        /// The page's title (default: the document's name).
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        /// A line under the title.
        #[arg(long, value_name = "TEXT")]
        subtitle: Option<String>,
        /// A footnote along the bottom of the page.
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// A legend swatch, `#rrggbb=what it means`; repeatable, in order.
        #[arg(long, value_name = "#RRGGBB=TEXT")]
        legend: Vec<String>,
        /// A note at a model point, `x,y,z=text` (model mm or written
        /// lengths), centred on where the point lands; repeatable.
        #[arg(long = "label", value_name = "X,Y,Z=TEXT", allow_hyphen_values = true)]
        labels: Vec<String>,
        /// Degrees clockwise from +y to north, for a plan's north arrow
        /// (default: the document's `render_defaults(north=)`, else +y).
        #[arg(long, value_name = "DEG")]
        north: Option<f64>,
        /// Paper for the drawing: letter, tabloid (the default, turned to
        /// suit the view), A4..A0, ARCH_A..ARCH_E.
        #[arg(long, value_name = "SIZE")]
        sheet: Option<String>,
        /// The drawing's scale, `1:48` or `1/4" = 1'-0"` (default: fitted to
        /// the page and snapped to a standard ratio).
        #[arg(long, value_name = "RATIO")]
        scale: Option<String>,
        /// The part of the view to draw, `x0,y0,x1,y1` in model mm of the
        /// view's frame (a plan's x and y; a section's along the cut and
        /// up): the page is fitted to it and nothing outside is drawn.
        #[arg(long, value_name = "X0,Y0,X1,Y1", allow_hyphen_values = true)]
        crop: Option<String>,
    },
    /// Lay the model out on print-ready plates and write one Bambu Studio
    /// project: every piece turned the way it prints with the least support,
    /// packed onto as few plates as hold it, each plate within the AMS's
    /// slots; bodies that lie on each other are one multi-material piece and
    /// bodies held captive across a small gap print in place. Prints the
    /// report: every decision with its numbers. Use this, not `export`, to
    /// make a print file from a model.
    Plate {
        #[command(flatten)]
        eval: EvalArgs,
        /// The project to write (.3mf).
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        /// The printer: bambu_h2d, bambu_h2c, bambu_h2s, bambu_x1c,
        /// bambu_x1e, bambu_p1s, bambu_p2s, bambu_a1, bambu_a1_mini (short
        /// forms work too). Default: the X1 Carbon.
        #[arg(long, value_name = "NAME")]
        printer: Option<String>,
        /// Filaments one plate may use (default 4).
        #[arg(long, value_name = "N")]
        ams: Option<u32>,
        /// A set of plates to make: colour or mono; repeatable, in order
        /// (default: colour).
        #[arg(long = "variant", value_name = "colour|mono")]
        variants: Vec<String>,
        /// Clear space between pieces, in mm (default 4).
        #[arg(long, value_name = "MM")]
        spacing: Option<f64>,
        /// The widest gap in mm across which captive bodies print in place
        /// (default 1).
        #[arg(long, value_name = "MM")]
        joint_gap: Option<f64>,
        /// The longest bridge in mm a flat downward face spans without
        /// support (default 10; 0 turns bridging off).
        #[arg(long, value_name = "MM")]
        max_bridge: Option<f64>,
        /// Plate only the bodies on this display layer; repeatable.
        #[arg(long = "layer", value_name = "NAME")]
        layers: Vec<String>,
        /// Bodies on this layer are test coupons, printed first on a plate
        /// of their own; repeatable.
        #[arg(long = "coupon-layer", value_name = "NAME")]
        coupon_layers: Vec<String>,
        /// Plate only these bodies, by id or name; repeatable.
        #[arg(long = "body", value_name = "ID")]
        bodies: Vec<String>,
        /// Also plate bodies on layers the script declares hidden.
        #[arg(long)]
        include_hidden: bool,
        /// Keep each object the script's own `export(..., name=)` calls make
        /// as one piece.
        #[arg(long)]
        script_objects: bool,
        /// Optional: a Bambu Studio project to take printer, process and
        /// filament settings from (only its settings; never its objects).
        #[arg(long, value_name = "FILE")]
        template: Option<String>,
        /// Also write an image of every plate and a contact sheet here.
        #[arg(long, value_name = "DIR")]
        plate_images: Option<PathBuf>,
        /// Print the plan as JSON instead of the report.
        #[arg(long)]
        json: bool,
    },
    /// Check whether a model can be printed, and say where the problems are.
    Check {
        #[command(flatten)]
        eval: EvalArgs,
        /// Machine to judge the model against: bambu_h2d, bambu_h2c,
        /// bambu_h2s, bambu_x1c, bambu_x1e, bambu_p1s, bambu_p2s, bambu_a1,
        /// bambu_a1_mini, prusa_mk4, ender3 (short forms work too).
        /// Supplies the bed, nozzle, layer height and material; every flag
        /// below overrides it.
        #[arg(long, value_name = "NAME")]
        printer: Option<String>,
        /// Minimum wall thickness in mm (default: nozzle x perimeters).
        #[arg(long, value_name = "MM")]
        min_wall: Option<f64>,
        /// Nozzle diameter in mm (default 0.4).
        #[arg(long, value_name = "MM")]
        nozzle: Option<f64>,
        /// Perimeters the slicer will print (default 2).
        #[arg(long, value_name = "N")]
        perimeters: Option<f64>,
        /// Support threshold in degrees from vertical (default 45).
        #[arg(long, value_name = "DEG")]
        max_overhang: Option<f64>,
        /// Layer height in mm (default: the printer's, or 0.2).
        #[arg(long, value_name = "MM")]
        layer_height: Option<f64>,
        /// Filament for the mass estimate: PLA, PETG, ABS, ASA, TPU, PA, PC.
        #[arg(long, value_name = "NAME")]
        material: Option<String>,
        /// Build volume in mm as X,Y,Z or one number for a cube (default 256).
        #[arg(long, value_name = "MM")]
        bed: Option<String>,
        /// Plate shape: rectangular (default) or circular, where --bed's first
        /// number is the diameter.
        #[arg(long, value_name = "SHAPE")]
        bed_shape: Option<String>,
        /// Also score build orientations and print the reasoning.
        #[arg(long)]
        orient: bool,
        /// The longest span in mm a flat downward face may bridge between
        /// two supports and count as a bridge, which slicers print without
        /// support, rather than as an overhang (default 10 mm, Bambu
        /// Studio's documented max bridge length; 0 counts every flat
        /// downward face as an overhang).
        #[arg(long, value_name = "MM", allow_hyphen_values = true)]
        max_bridge: Option<f64>,
        /// The largest connected overhang patch in mm² that prints without
        /// support (default 4; 0 counts every patch).
        #[arg(long, value_name = "MM2")]
        max_overhang_area: Option<f64>,
        /// How far a near-level ledge held up on one side may stick out, in
        /// mm (default 3; 0 judges every ledge).
        #[arg(long, value_name = "MM")]
        max_cantilever: Option<f64>,
        /// The widest round horizontal hole in mm whose top is bridged; a
        /// wider one wants a teardrop top (default 7; 0 flags every one).
        #[arg(long, value_name = "MM")]
        max_horizontal_hole: Option<f64>,
        /// How deep in mm a region starting in mid-air may hang before it
        /// is an island (default one layer; 0 flags every one).
        #[arg(long, value_name = "MM")]
        min_island: Option<f64>,
        /// Height over width past which a feature under --slender-width is
        /// a warning (default 8; 0 turns it off).
        #[arg(long, value_name = "RATIO")]
        max_slenderness: Option<f64>,
        /// What counts as slender, in mm across (default 3).
        #[arg(long, value_name = "MM")]
        slender_width: Option<f64>,
        /// Height over the square root of the first-layer area past which
        /// the part may tip or lift, a warning (default 6; 0 turns it off).
        #[arg(long, value_name = "RATIO")]
        max_height_to_base: Option<f64>,
        /// Judge only whether it prints without support: report the
        /// overhang, island, cantilever, horizontal_hole and bridge
        /// findings, and exit 1 when there is any.
        #[arg(long)]
        support_free: bool,
        /// Also warn when a body stands in a clearly worse orientation for
        /// printing than the best `cadpole plate` finds for it (more than
        /// --orientation-tolerance mm² of support equivalent worse).
        #[arg(long)]
        orientation: bool,
        /// How much worse a placement may be before --orientation warns, in
        /// mm² of support equivalent (default 5).
        #[arg(long, value_name = "MM2")]
        orientation_tolerance: Option<f64>,
        /// Check only the bodies on this display layer, by the name
        /// `layer(...)` declared (`other` is the bodies on no layer);
        /// repeatable. A hidden layer named here is checked. Adds to --body.
        #[arg(long = "layer", value_name = "NAME")]
        layers: Vec<String>,
        /// Check only this body, by id (`feature#n`, as `cadpole eval` lists
        /// them) or by name; repeatable. Adds to --layer.
        #[arg(long = "body", value_name = "ID")]
        bodies: Vec<String>,
        /// Also check the bodies on display layers the script declares
        /// hidden; by default they are left out, as `cadpole export` leaves
        /// them out.
        #[arg(long)]
        include_hidden: bool,
        /// Print the whole report as JSON instead of a summary.
        #[arg(long)]
        json: bool,
    },
    /// Run the tests a script declares with `test(name, fn, ...)`. Each runs
    /// after the model is built; a `sweep=` re-evaluates the model at every
    /// listed parameter value and names the value that failed. Exit 1 when
    /// any test fails or the model does not build.
    Test {
        #[command(flatten)]
        eval: EvalArgs,
        /// Run only this test, by the name given to `test(...)`; repeatable.
        #[arg(long, value_name = "NAME")]
        name: Vec<String>,
        /// Sweep points evaluated at once (default: the machine's cores, up
        /// to 8). Never changes the result.
        #[arg(long)]
        threads: Option<u32>,
        /// Print the results as JSON instead of a summary.
        #[arg(long)]
        json: bool,
    },
    /// Inspect the geometry body by body -- faces, edges, triangles, the
    /// solid validator's verdict with every failing check located, folds,
    /// the thinnest triangle, manifold's own self-intersection verdict --
    /// and then what the kernel decided while making it, grouped by feature:
    /// which engine ran a boolean, whether a cleanup stopped early, whether a
    /// tessellation moved off the requested tolerance.
    ///
    /// The place to start when a model looks wrong, has lost its face names,
    /// or a boolean came back with far more faces than it should have.
    Inspect {
        #[command(flatten)]
        eval: EvalArgs,
        /// Body id to inspect (`feature#n`); repeatable. Default: every
        /// visible body.
        #[arg(long = "body", value_name = "ID")]
        bodies: Vec<String>,
        /// Print the whole report as JSON instead of tables.
        #[arg(long)]
        json: bool,
    },
    /// Evaluate a document ahead of time and write the result as a
    /// A baked model: the evaluated model -- every body's mesh, names,
    /// materials, layers, parameters -- as a zlib-deflated
    /// `cadpole.v1.BakedModel`, which the web app opens read-only without
    /// running Starlark (`?baked=<url>`, or the page `cadpole publish` writes).
    /// The script's sources are not in it unless --include-sources.
    Bake {
        #[command(flatten)]
        eval: EvalArgs,
        /// Where to write the bake (conventionally `<name>.` and the baked-model extension).
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
        #[command(flatten)]
        bake: BakeArgs,
    },
    /// Publish a document as a static website: bake it (see `cadpole bake`)
    /// and write it into --out-dir beside a copy of Cadpole's web app that
    /// opens it at load time, read-only -- orbit, the feature tree, layers,
    /// selection and the inspector, with no server and no Starlark in the
    /// browser. Serve the directory from any static host. File names carry
    /// content hashes; the wasm engine, the service worker and the script's
    /// sources are left out (--include-sources puts the sources in).
    Publish {
        #[command(flatten)]
        eval: EvalArgs,
        /// The directory to write the site into. Created if missing; an
        /// earlier publish there is replaced. A directory holding anything
        /// else is refused without --force.
        #[arg(long, short = 'o', visible_alias = "out", value_name = "DIR")]
        out_dir: PathBuf,
        #[command(flatten)]
        bake: BakeArgs,
        /// Publish into a non-empty directory that holds no earlier
        /// publish. Adds the site's files; deletes nothing.
        #[arg(long)]
        force: bool,
        /// The built web app to copy (default: the one this binary
        /// carries, `//src/web:dist`).
        #[arg(long, value_name = "DIR")]
        web_dir: Option<PathBuf>,
    },
    /// Print the Starlark API reference (markdown, or JSON with --json).
    Doc {
        #[arg(long)]
        json: bool,
    },
    /// Serve the web UI and the session API for one document.
    Serve(server::ServeArgs),
    /// Print session events as JSON lines (from a running `cadpole serve`).
    ///
    /// Without a mode flag it prints the events already in the log and
    /// returns. `--next` waits, silently, for the next event worth acting on,
    /// prints it and exits; `--follow` does the same without exiting. Both
    /// print nothing while nothing happens, so a program that watches this
    /// command's output is woken only when there is something to do.
    Listen {
        /// The `cadpole serve` to read: `unix:/path/to.sock` or
        /// `http://host:port`. Default: the running server, found in the
        /// runtime directory.
        #[arg(long, alias = "server", env = "CADPOLE_SERVER")]
        url: Option<String>,
        #[arg(long, env = "CADPOLE_TOKEN")]
        token: Option<String>,
        /// Only events with seq greater than this. Default: everything in the
        /// log; with --next or --follow, where this client last stopped
        /// reading, or the end of the log for a client the server has not
        /// seen.
        #[arg(long)]
        after: Option<u64>,
        /// Wait for the next event worth acting on, print it and exit.
        /// Run it again to wait for the one after: the server keeps the
        /// position.
        #[arg(long, conflicts_with = "follow")]
        next: bool,
        /// Keep printing events as they happen instead of returning after
        /// the current backlog.
        #[arg(long)]
        follow: bool,
        /// Only these kinds of event, comma-separated (`message`,
        /// `selection_changed`, `params_changed`, `script_changed`,
        /// `client_failure`, ...). Default with --next or --follow: a
        /// `message` typed by the user, and a `selection_changed` that
        /// carries a note. Default otherwise: every kind.
        #[arg(long, value_delimiter = ',', conflicts_with = "all")]
        kinds: Vec<String>,
        /// Every kind of event, also with --next or --follow.
        #[arg(long)]
        all: bool,
        /// With --next or --follow: print events even while the UI's Listen
        /// toggle is off.
        #[arg(long)]
        always_listen: bool,
        /// With --next or --follow: the id the server keeps this reader's
        /// position under. Give each of several watchers of one server its
        /// own.
        #[arg(long, env = "CADPOLE_LISTEN_CLIENT", default_value = "listen")]
        client: String,
    },
    /// Print what the user has selected in the UI.
    Selection {
        /// The `cadpole serve` to read: `unix:/path/to.sock` or
        /// `http://host:port`. Default: the running server, found in the
        /// runtime directory.
        #[arg(long, alias = "server", env = "CADPOLE_SERVER")]
        url: Option<String>,
        #[arg(long, env = "CADPOLE_TOKEN")]
        token: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Call any operation by name: `cadpole op --list`, `cadpole op Render --width 800`.
    ///
    /// Operations and their flags are generated from `service CadpoleOps` in
    /// proto/cadpole/v1/ops.proto, so this covers everything Cadpole can do,
    /// including anything the ergonomic verbs above do not wrap yet.
    #[command(disable_help_flag = true)]
    Op {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// Run as an MCP server (stdio) for assistants; also a Claude Code channel.
    /// See docs/assistant-integration.md.
    Mcp(mcp::McpArgs),
    /// Run a document's pending optimisation solves to completion.
    ///
    /// A solve too long for one evaluation -- 120 s in the CLI, 20 s in `cadpole
    /// serve` -- or one written `solve(job=True)` is reported pending. This
    /// evaluates the document, runs every such solve as a job (its own
    /// `cadpole` process, several at once, each under a memory limit), waits,
    /// and evaluates again with their answers, until nothing is pending: a
    /// chain of solves, floor by floor, runs one link a round. Answers go in
    /// the solution cache, so every later `cadpole eval`, `render` or `serve`
    /// of the document reads them. Exit 1 when anything is still pending or
    /// a job failed.
    Solve {
        #[command(flatten)]
        eval: EvalArgs,
        /// Jobs at once (default: a quarter of the cores, from 1 to 4).
        #[arg(long, value_name = "N")]
        jobs: Option<usize>,
        /// Each job's time limit, in seconds, for solves that give no
        /// `time_limit=` of their own (default: none).
        #[arg(long, value_name = "SECONDS")]
        time_limit: Option<f64>,
        /// Each job's memory limit, in MB (default 8192; 0 for none).
        #[arg(long, value_name = "MB")]
        mem_mb: Option<u64>,
        /// Stop waiting after this many seconds; the jobs carry on and file
        /// their answers when they land.
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<f64>,
        /// Start the jobs and return at once.
        #[arg(long)]
        detach: bool,
        /// Run again the solves whose last job failed, was cancelled, or
        /// stopped at a host's time limit.
        #[arg(long)]
        rerun: bool,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// The optimisation solve jobs on this machine: `cadpole jobs`
    /// lists them, `cadpole jobs wait` waits for them, `cadpole jobs cancel` stops
    /// them. Jobs started by `cadpole serve`, `cadpole solve` and `cadpole mcp` alike.
    #[command(args_conflicts_with_subcommands = true)]
    Jobs {
        #[command(subcommand)]
        action: Option<JobsCmd>,
        /// Include jobs that ended more than an hour ago (kept a week).
        #[arg(long)]
        all: bool,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// One solve job's process: `cadpole serve` and `cadpole solve` start
    /// it, with the solution cache's directory and the job's id; it solves
    /// the problem its parent wrote and files the answer.
    #[command(hide = true, name = "opt-job")]
    OptJob {
        root: PathBuf,
        id: String,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// List jobs: running and queued, and those that ended in the last hour.
    List {
        /// Only these jobs.
        ids: Vec<String>,
        /// Include jobs that ended more than an hour ago.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Wait until jobs end (default: every job running or queued now).
    Wait {
        ids: Vec<String>,
        /// Give up after this many seconds (default: wait until they end).
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<f64>,
        #[arg(long)]
        json: bool,
    },
    /// Stop jobs: a queued one never starts, a running one is terminated.
    Cancel {
        ids: Vec<String>,
        /// Every job running or queued.
        #[arg(long)]
        all: bool,
        /// Delete their records too, so the next evaluation runs those
        /// solves again.
        #[arg(long)]
        forget: bool,
        #[arg(long)]
        json: bool,
    },
}

/// How this CLI process runs solve jobs.
fn local_job_settings(verb: &str, jobs: Option<usize>, time_limit: Option<f64>, mem_mb: Option<u64>) -> opt_jobs::jobs::Settings {
    let mut s = opt_jobs::jobs::Settings::new(format!("cadpole {verb} (pid {})", std::process::id()));
    if let Some(n) = jobs.filter(|n| *n > 0) {
        s.parallel = n;
    }
    s.time_limit = time_limit.filter(|t| *t > 0.0);
    if let Some(m) = mem_mb {
        s.mem_mb = m;
    }
    s.when_landed = format!("`cadpole {verb}` evaluates the document again when it lands");
    s
}

/// A number for a table: six decimals at most, trailing zeros dropped, so a
/// MILP's 2.9999999999999987 reads 3.
fn short_num(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// One line per job, for `cadpole solve` and `cadpole jobs` (stdout), and `cadpole
/// eval --jobs wait` (stderr, which keeps stdout the evaluation's).
fn print_jobs(jobs: &[pb::SolveJob]) {
    for line in job_lines(jobs) {
        println!("{line}");
    }
}

fn job_lines(jobs: &[pb::SolveJob]) -> Vec<String> {
    let mut out = Vec::new();
    for j in jobs {
        let objective = j.objective.map(|o| format!("obj {}", short_num(o))).unwrap_or_default();
        let gap = j.gap.filter(|g| *g > 0.0).map(|g| format!("gap {:.2}%", g * 100.0)).unwrap_or_default();
        out.push(format!(
            "{}  {:<9} {:<10} {:<12} {:<11} {:>7.1} s  {}",
            j.id,
            j.state,
            if j.status.is_empty() { "-" } else { j.status.as_str() },
            objective,
            gap,
            j.elapsed_s,
            j.origin
        ));
        if !j.message.is_empty() && j.state != "done" {
            out.push(format!("              {}", j.message));
        }
    }
    out
}

/// `--size WxH`, both at least one pixel: a 0 would travel as the proto's
/// "unset" and quietly become the engine's default for that side.
fn parse_size(s: &str) -> anyhow::Result<(u32, u32)> {
    let bad = || anyhow::anyhow!("--size must be WxH in pixels, both above 0, got {s:?}");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let (w, h): (u32, u32) = (w.trim().parse().map_err(|_| bad())?, h.trim().parse().map_err(|_| bad())?);
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

/// `cadpole check --layer` / `cadpole export --layer` take a layer's name.
/// `cadpole render --layer` takes `NAME=on|off|<opacity>`, so a
/// value with `=` in it is that flag's spelling typed at the wrong command,
/// and is refused with what this one wants rather than looked up as a name.
fn layer_names_arg(layers: &[String]) -> anyhow::Result<Vec<String>> {
    layers
        .iter()
        .map(|l| {
            if l.contains('=') {
                anyhow::bail!(
                    "--layer {l:?}: here --layer takes a layer's name and selects its bodies; NAME=on|off is \
                     `cadpole render --layer` (and `cadpole export --view/--section`), which switches a layer in a picture"
                );
            }
            Ok(l.trim().to_owned())
        })
        .collect()
}

/// `--background #rrggbb` as the request's linear colour.
///
/// The flag is written in sRGB, the way a page or a slide names its colour,
/// and the renderer writes uncovered pixels as `to_srgb8(background)`, so
/// the bytes are decoded the same way a script's `material(color="#…")` is
/// (`paint::unhex`) for the pixel that comes out to be the one typed. Unlike
/// `unhex`, which greys a bad string because a colour is never worth failing
/// a build, a flag that does not parse fails the command.
fn background_color(s: &str) -> anyhow::Result<pb::Color> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("--background must be #rrggbb or transparent, got {s:?}");
    }
    let [r, g, b] = cadpole_engine::cadpole_core::paint::unhex(hex);
    Ok(pb::Color { r, g, b })
}

/// `--variant NAME=WxH`.
fn parse_variant(s: &str) -> anyhow::Result<(Option<String>, (u32, u32))> {
    let (name, size) = s.split_once('=').ok_or_else(|| anyhow::anyhow!("--variant takes NAME=WxH, got {s:?}"))?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("--variant: the name becomes part of a file name, so letters, digits, - and _ only; got {name:?}");
    }
    Ok((Some(name.to_owned()), parse_size(size.trim())?))
}

/// Where a variant's output goes: a directory under `out` for frames, and
/// otherwise `out` with `_NAME` before its extension. No variant, `out`.
fn variant_path(out: &std::path::Path, variant: Option<&str>, directory: bool) -> PathBuf {
    let Some(v) = variant else { return out.to_path_buf() };
    if directory {
        return out.join(v);
    }
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("animation");
    let name = match out.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}_{v}.{ext}"),
        None => format!("{stem}_{v}"),
    };
    out.with_file_name(name)
}

/// `--background transparent` (or `none`): no background, coverage alpha.
fn is_transparent(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "transparent" | "none")
}

/// `--supersample N`, refused outside 1..4 rather than clamped: 5 would
/// quietly be 4 and 0 quietly 1.
fn supersample(view: &ViewArgs) -> anyhow::Result<u32> {
    match view.supersample {
        None => Ok(0),
        Some(n) if (1..=4).contains(&n) => Ok(n),
        Some(n) => anyhow::bail!("--supersample must be 1, 2, 3 or 4, got {n}"),
    }
}

/// The sidecar `--camera-json` writes (docs/views.md, "The camera
/// sidecar"): for each frame the camera as a position and as matrices, the
/// ground plane, and the model's bounding box in the world and in pixels.
fn camera_json(cameras: &[pb::CameraInfo]) -> String {
    let v = |p: &Option<pb::Vec3>| {
        let p = p.clone().unwrap_or_default();
        serde_json::json!([p.x, p.y, p.z])
    };
    let frames: Vec<serde_json::Value> = cameras
        .iter()
        .map(|c| {
            serde_json::json!({
                "index": c.index,
                "t": c.t,
                "eye": v(&c.eye),
                "target": v(&c.target),
                "up": v(&c.up),
                "fov_degrees": c.fov_degrees,
                "orthographic": c.orthographic,
                "ortho_height": c.ortho_height,
                "shift": c.shift,
                "near": c.near,
                "far": c.far,
                "view": c.view,
                "projection": c.projection,
                "ground": {
                    "origin": v(&c.ground_origin),
                    "normal": v(&c.ground_normal),
                    "u": v(&c.ground_u),
                    "v": v(&c.ground_v),
                },
                "bounds": {
                    "min": v(&c.bounds_min),
                    "max": v(&c.bounds_max),
                    "px": c.bounds_px,
                },
            })
        })
        .collect();
    let (width, height) = cameras.first().map(|c| (c.width, c.height)).unwrap_or((0, 0));
    let doc = serde_json::json!({
        "width": width,
        "height": height,
        "units": "mm",
        "matrices": "row-major 4x4 on column vectors: clip = projection * view * (x, y, z, 1); \
                     ndc = clip.xyz / clip.w; x_px = (ndc.x + 1) / 2 * width; y_px = (1 - ndc.y) / 2 * height; \
                     pixel (0, 0) is the top-left corner",
        "frames": frames,
    });
    serde_json::to_string_pretty(&doc).expect("a JSON value serialises") + "\n"
}

fn write_camera_json(path: &std::path::Path, cameras: &[pb::CameraInfo]) -> anyhow::Result<()> {
    std::fs::write(path, camera_json(cameras)).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("wrote {} ({} camera{})", path.display(), cameras.len(), if cameras.len() == 1 { "" } else { "s" });
    Ok(())
}

fn overrides(params: &[String]) -> anyhow::Result<HashMap<String, ParamValue>> {
    params.iter().map(|p| cadpole_engine::parse_override(p)).collect()
}

/// The document named by `--doc`/the positional input, with `--param`
/// overrides folded into it. Requests that have no override map of their own
/// (Evaluate, Export) get them this way; the engine merges a document's
/// overrides with a request's identically.
fn document(eval: &EvalArgs) -> anyhow::Result<pb::Document> {
    let mut doc = input::load(&eval.input)?;
    for (k, v) in override_map(&eval.params)? {
        doc.param_overrides.insert(k, v);
    }
    Ok(doc)
}

/// `cadpole export --template`: a path to a saved Bambu Studio project
/// is added to the document's assets under its file name, so the request
/// can name it as `ExportRequest.template`; anything else names an asset the
/// document already has. Returns the asset name.
fn template_asset(doc: &mut pb::Document, template: &str) -> anyhow::Result<String> {
    let path = std::path::Path::new(template);
    if !path.is_file() {
        return Ok(template.to_owned());
    }
    let bytes = std::fs::read(path).with_context(|| format!("--template: reading {}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "template.3mf".to_owned());
    let record = cadpole_engine::cadpole_assets::Asset::describe(&name, &bytes);
    match doc.assets.iter().find(|a| a.name == name) {
        Some(a) if a.sha256 == record.sha256 => {}
        Some(_) => anyhow::bail!(
            "--template {}: the document already has a different asset named {name:?}; rename the file",
            path.display()
        ),
        None => doc.assets.push(cadpole_engine::asset_to_proto(&record, bytes)),
    }
    Ok(name)
}

/// Parameter overrides as the proto map every request carries.
fn override_map(params: &[String]) -> anyhow::Result<HashMap<String, pb::ParamValue>> {
    Ok(overrides(params)?.into_iter().map(|(k, v)| (k, v.to_proto())).collect())
}

/// `--layer NAME=on|off|OPACITY` flags as request fields.
fn layer_states(specs: &[String]) -> anyhow::Result<Vec<pb::LayerState>> {
    specs.iter().map(|s| cadpole_engine::parse_layer_state(s)).collect()
}

/// Run one operation through the registry and decode its typed response.
///
/// Every verb goes through here, so the CLI cannot do anything MCP and HTTP
/// cannot: the three front-ends are the same operations with different skins.
fn call<Resp: prost::Message + Default>(
    method: &str,
    request: &impl prost::Message,
    host: &dyn Host,
) -> anyhow::Result<Resp> {
    let bytes = cadpole_ops::call_proto(method, &request.encode_to_vec(), host)?;
    Ok(Resp::decode(bytes.as_slice())?)
}

/// A PNG from an operation's response, back as an `Image`.
///
/// Operations return encoded images, so anything the CLI composes out of
/// several renders (`--compare-edges`) decodes them again rather than
/// reaching around the registry into the renderer.
fn image_from_png(bytes: &[u8]) -> anyhow::Result<Image> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
        "expected 8-bit RGBA PNG, got {:?}/{:?}",
        info.color_type,
        info.bit_depth
    );
    buf.truncate(info.buffer_size());
    Ok(Image { width: info.width, height: info.height, rgba: buf })
}

/// One labelled tile per edge style (plus the crease-angle sweep), so the
/// default overlay can be judged side by side. See docs/views.md.
///
/// Each tile is one `Render` operation, so what is compared is the request
/// fields an assistant would set, not renderer internals.
///
/// `None` when the document has errors, which are printed: there is no
/// sheet of a model that did not build.
fn edge_variants(base: &pb::RenderRequest, host: &dyn Host) -> anyhow::Result<Option<Image>> {
    let creases = |deg: f64| pb::RenderRequest {
        edge_mode: pb::EdgeMode::Creases as i32,
        edge_crease_degrees: deg,
        ..base.clone()
    };
    let variants: Vec<(String, pb::RenderRequest)> = vec![
        ("EDGES OFF".into(), pb::RenderRequest { edge_mode: pb::EdgeMode::Off as i32, ..base.clone() }),
        ("SILHOUETTE".into(), pb::RenderRequest { edge_mode: pb::EdgeMode::Silhouette as i32, ..base.clone() }),
        ("CREASES 20".into(), creases(20.0)),
        ("CREASES 30".into(), creases(30.0)),
        ("CREASES 45".into(), creases(45.0)),
        ("CREASES 30 NO OUTLINE".into(), pb::RenderRequest { no_silhouettes: true, ..creases(30.0) }),
        ("ALL SEAMS".into(), pb::RenderRequest { edge_mode: pb::EdgeMode::All as i32, ..base.clone() }),
    ];
    let mut tiles = Vec::with_capacity(variants.len());
    for (k, (label, req)) in variants.into_iter().enumerate() {
        let resp: pb::RenderResponse = call("Render", &req, host)?;
        // Every tile evaluates the same document: its diagnostics once.
        if k == 0 && print_diagnostics(&resp.diagnostics) {
            return Ok(None);
        }
        tiles.push((label, image_from_png(&resp.png)?));
    }
    Ok(Some(if base.transparent {
        contact_sheet_clear(&tiles, 4, 1, [230, 232, 236, 255])
    } else {
        contact_sheet(&tiles, 4, RenderOptions::default().background)
    }))
}

/// `--edges off|silhouette|creases|all` as the request enum; unset means the
/// engine default (creases).
fn edge_mode(s: Option<&str>) -> pb::EdgeMode {
    let Some(s) = s else { return pb::EdgeMode::Unspecified };
    match EdgeMode::parse(s) {
        Some(EdgeMode::Off) => pb::EdgeMode::Off,
        Some(EdgeMode::Silhouette) => pb::EdgeMode::Silhouette,
        Some(EdgeMode::Creases) => pb::EdgeMode::Creases,
        Some(EdgeMode::All) => pb::EdgeMode::All,
        None => {
            eprintln!("warning: unknown --edges {s:?}, using the default");
            pb::EdgeMode::Unspecified
        }
    }
}

/// `--grid off|finite|infinite` as the request enum.
fn grid_mode(s: Option<&str>) -> pb::GridMode {
    let Some(s) = s else { return pb::GridMode::Unspecified };
    match GridMode::parse(s) {
        Some(GridMode::Off) => pb::GridMode::Off,
        Some(GridMode::Finite) => pb::GridMode::Finite,
        Some(GridMode::Infinite) => pb::GridMode::Infinite,
        None => {
            eprintln!("warning: unknown --grid {s:?}, using the default");
            pb::GridMode::Unspecified
        }
    }
}

/// `--view PRESET`, or `--eye x,y,z [--target x,y,z] [--up x,y,z]` for a
/// free camera. An empty preset with no eye defers to the document's
/// `render_defaults(camera=)`, else the engine fits `iso`.
fn camera(view: &ViewArgs) -> anyhow::Result<Option<pb::Camera>> {
    // 0 is the request's "unset", so an explicit 0 (or NaN) would quietly be
    // the preset's lens, and 180 or more sees nothing.
    let fov_degrees = match view.fov {
        None => 0.0,
        Some(f) if f.is_finite() && f > 0.0 && f < 180.0 => f,
        Some(f) => anyhow::bail!("--fov {f}: a vertical field of view in degrees, above 0 and below 180"),
    };
    let shift = match view.shift {
        None => 0.0,
        Some(s) if s.is_finite() && s.abs() <= 2.0 => s,
        Some(s) => anyhow::bail!("--shift {s}: a fraction of the frame height, -2 to 2"),
    };
    let Some(eye) = &view.eye else {
        let preset = view.view.clone().unwrap_or_default();
        view_preset(&preset)?;
        return Ok(Some(pb::Camera { preset, fov_degrees, shift, level: view.level, ..Default::default() }));
    };
    let eye = parse_xyz(eye, "--eye")?;
    let target = view.target.as_deref().map(|t| parse_xyz(t, "--target")).transpose()?;
    let up = view.up.as_deref().map(|u| parse_xyz(u, "--up")).transpose()?;
    if let Some(t) = &target {
        if (t.x - eye.x).abs() + (t.y - eye.y).abs() + (t.z - eye.z).abs() <= 0.0 {
            anyhow::bail!("--eye and --target coincide");
        }
    }
    Ok(Some(pb::Camera { preset: String::new(), eye: Some(eye), target, up, fov_degrees, shift, level: view.level, ..Default::default() }))
}

/// `--view`: a preset the renderer knows or a finite `az=,el=` pair. The
/// renderer draws an unknown name as `iso` (`Camera::preset`), so a typo
/// would come back as a valid-looking picture of the wrong view.
fn view_preset(preset: &str) -> anyhow::Result<()> {
    use cadpole_engine::cadpole_core::render::Camera;
    // The angular parser reads a NaN angle as an absent one (0), so the
    // values are checked here too.
    let finite = preset.split(',').all(|part| match part.split_once('=') {
        Some((_, v)) => v.trim().parse::<f64>().is_ok_and(f64::is_finite),
        None => true,
    });
    match Camera::preset_direction(preset) {
        Some(d) if finite && d.x.is_finite() && d.y.is_finite() && d.z.is_finite() => Ok(()),
        _ => anyhow::bail!(
            "--view {preset:?}: one of iso, front, back, top, bottom, left, right, iso-bottom, iso-back, product, \
             hero, or az=<deg>,el=<deg> with finite angles"
        ),
    }
}

/// `--exposure`, a multiplier above 0 (0 is the request's "unset"; inf
/// paints the model black).
fn exposure_arg(view: &ViewArgs) -> anyhow::Result<f64> {
    match view.exposure {
        None => Ok(0.0),
        Some(e) if e.is_finite() && e > 0.0 => Ok(e as f64),
        Some(e) => anyhow::bail!("--exposure {e}: a finite multiplier above 0 (1 is unchanged)"),
    }
}

/// `x,y,z` as a proto vector.
fn parse_xyz(s: &str, what: &str) -> anyhow::Result<pb::Vec3> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        anyhow::bail!("{what} must be x,y,z, got {s:?}");
    }
    let mut v = [0.0f64; 3];
    for (k, p) in parts.iter().enumerate() {
        v[k] = p.parse().map_err(|_| anyhow::anyhow!("{what}: {p:?} is not a number"))?;
        if !v[k].is_finite() {
            anyhow::bail!("{what}: {p:?} is not finite");
        }
    }
    Ok(pb::Vec3 { x: v[0], y: v[1], z: v[2] })
}

/// The instants a shadow study draws, and the sheet's column
/// count. `--at YYYY-MM-DD,HH:MM`, given one or more times, is that list
/// in that order — the winter solstice at noon, the equinox at 9 and at 15
/// — as one row up to four across, then a near-square grid; otherwise the
/// grid of `--dates` (default the June solstice, the March equinox and the
/// December solstice of the year `--sun` names) by `--times` (default
/// 09:00, 12:00, 15:00), a row per date. `--sun` names the place and the
/// zone either way; its own date and time are unused.
fn shadow_study_instants(sun: &str, dates: Option<&str>, times: Option<&str>, at: &[String]) -> anyhow::Result<(solar::Site, Vec<(solar::Date, f64)>, u32)> {
    let Some((site, date, _)) = solar::parse_site_spec(sun).map_err(|e| anyhow::anyhow!(e))? else {
        anyhow::bail!("--shadow-study needs --sun lat,lon,YYYY-MM-DD,HH:MM[,tz]: the place and the year; got {sun:?}");
    };
    if !at.is_empty() {
        let mut instants = Vec::with_capacity(at.len());
        for a in at {
            let Some((d, t)) = a.split_once(',') else {
                anyhow::bail!("--at: expected YYYY-MM-DD,HH:MM, got {a:?}");
            };
            let date = solar::Date::parse(d.trim()).map_err(|e| anyhow::anyhow!("--at {a:?}: {e}"))?;
            let hours = solar::parse_time(t.trim()).map_err(|e| anyhow::anyhow!("--at {a:?}: {e}"))?;
            instants.push((date, hours));
        }
        if instants.len() > 36 {
            anyhow::bail!("--at takes at most 36 instants");
        }
        let n = instants.len();
        let mut cols = n.min(4);
        if n > 4 {
            cols = 1;
            while cols * cols < n {
                cols += 1;
            }
        }
        return Ok((site, instants, cols as u32));
    }
    let dates: Vec<solar::Date> = match dates {
        None => {
            let [june, december] = solar::solstices(date.year);
            let [march, _] = solar::equinoxes(date.year);
            vec![june, march, december]
        }
        Some(s) => s
            .split(',')
            .map(|d| solar::Date::parse(d.trim()).map_err(|e| anyhow::anyhow!("--dates: {e}")))
            .collect::<anyhow::Result<_>>()?,
    };
    let times: Vec<f64> = times
        .unwrap_or("09:00,12:00,15:00")
        .split(',')
        .map(|t| solar::parse_time(t.trim()).map_err(|e| anyhow::anyhow!("--times: {e}")))
        .collect::<anyhow::Result<_>>()?;
    if dates.is_empty() || dates.len() > 12 {
        anyhow::bail!("--dates takes between one and twelve dates");
    }
    if times.is_empty() || times.len() > 12 {
        anyhow::bail!("--times takes between one and twelve times of day");
    }
    let mut instants = Vec::with_capacity(dates.len() * times.len());
    for d in &dates {
        for &h in &times {
            instants.push((*d, h));
        }
    }
    Ok((site, instants, times.len() as u32))
}

/// `cadpole render --shadow-study`: one render per instant of
/// [`shadow_study_instants`] — nine by default, the June solstice, the
/// March equinox and the December solstice of the year `--sun` names at
/// 09:00, 12:00 and 15:00 — into one labelled contact sheet, every tile
/// through the same `Render` operation with the same fitted camera.
///
/// `None` when the document has errors, which are printed.
fn shadow_study_sheet(base: &pb::RenderRequest, sun: &str, dates: Option<&str>, times: Option<&str>, at: &[String], host: &dyn Host) -> anyhow::Result<Option<Image>> {
    let (site, instants, cols) = shadow_study_instants(sun, dates, times, at)?;
    // The label is short enough for a 480 px tile at the sheet's 12 px
    // font: `JUN 21 09:00  az 95 el 40`.
    const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
    let mut tiles = Vec::with_capacity(instants.len());
    for (d, h) in instants {
        let spec = SunSpec::at(&site, d, h);
        let req = pb::RenderRequest {
            sun: format!("{},{},{d},{},{}", site.latitude, site.longitude, solar::format_time(h), site.tz_hours),
            ..base.clone()
        };
        let resp: pb::RenderResponse = call("Render", &req, host)?;
        if print_diagnostics(&resp.diagnostics) {
            return Ok(None);
        }
        let img = image_from_png(&resp.png)?;
        let when = format!("{} {} {}", MONTHS[(d.month as usize - 1).min(11)], d.day, solar::format_time(h));
        let label = if spec.is_up() {
            format!("{when}  az {:.0} el {:.0}", spec.azimuth, spec.elevation)
        } else {
            format!("{when}  below the horizon")
        };
        tiles.push((label, img));
    }
    Ok(Some(if base.transparent {
        contact_sheet_clear(&tiles, cols, 2, [230, 232, 236, 255])
    } else {
        contact_sheet_with(&tiles, cols, [0.09, 0.09, 0.1], 2, [230, 232, 236, 255])
    }))
}

/// `--backend raster|path`.
///
/// `gpu` is deliberately absent from this mapping: it is a property of the
/// *host*, not of the request, so it never travels in a
/// `RenderRequest` and `RenderBackend` gains no value for it. `cadpole render
/// --backend gpu` runs [`render_gpu`] locally instead and falls back to the
/// raster backend when the machine has no adapter.
fn render_backend(name: &str) -> anyhow::Result<pb::RenderBackend> {
    Ok(match name {
        "path" | "path_trace" | "pathtrace" => pb::RenderBackend::PathTrace,
        "raster" => pb::RenderBackend::Raster,
        n if wants_gpu(n) => pb::RenderBackend::Raster,
        other => anyhow::bail!("--backend {other:?}: raster, gpu or path"),
    })
}

/// Why `cadpole animate` refuses a view flag it shares with `render`, if it
/// does: `AnimateRequest` has no analysis or debug view, and the frames are
/// drawn by the engine's CPU renderers, never the GPU one.
fn animate_refuses(view: &ViewArgs) -> Option<String> {
    let flag = if view.analysis.is_some() {
        "--analysis"
    } else if view.debug.is_some() {
        "--debug"
    } else if wants_gpu(&view.backend) {
        "--backend gpu"
    } else {
        return None;
    };
    Some(format!("animate: {flag} is a `cadpole render` view; animate draws its frames with the raster or path backend and no analysis or debug colouring"))
}

fn wants_gpu(name: &str) -> bool {
    matches!(name, "gpu" | "wgpu" | "viewport")
}

/// What `cadpole render --backend gpu` draws, prepared on the CPU: the
/// scene, its camera and options, and the diagnostics the response carries.
struct GpuScene {
    scene: cadpole_engine::RenderScene,
    camera: cadpole_engine::cadpole_core::render::Camera,
    opts: RenderOptions,
    model: pb::Model,
    diagnostics: Vec<pb::Diagnostic>,
}

/// What `--backend gpu` does with a request: draw this scene, or hand the
/// request to the CPU rasterizer for the reason given (said as a note).
enum GpuPlan {
    Cpu(String),
    Draw(GpuScene),
}

/// Evaluate a render request and prepare the scene the wgpu renderer draws,
/// through the same steps as the engine's `Render` (`cadpole_engine::render`):
/// the request's limits, its look (a misspelt one is an error), the
/// document's `render_defaults(...)`, the `--section` cut and the camera the
/// request or the document names. Only drawing differs.
///
/// The wgpu renderer has no sun, so a sun -- the request's or the document's -- is the CPU's to draw;
/// that is known only once the document has been evaluated.
fn gpu_scene(req: &pb::RenderRequest) -> anyhow::Result<GpuPlan> {
    let mut req = req.clone();
    let mut diagnostics = cadpole_engine::limits::clamp_render(&mut req);
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("render: no document"))?;
    let look = cadpole_engine::Look::from_render(&req)?;
    if look.sun.is_some() {
        return Ok(GpuPlan::Cpu("--sun".into()));
    }
    let extra = cadpole_engine::overrides_from_proto(&req.param_overrides);
    let (mut ctx, ms) = cadpole_engine::evaluate_document_with(doc, &extra, req.tolerance, req.boolean_tolerance);
    let mut opts = cadpole_engine::render_options_from_proto(&req);
    cadpole_engine::apply_document_defaults(&mut opts, &look, ctx.render_defaults.as_ref());
    if opts.sun.is_some() {
        return Ok(GpuPlan::Cpu("the document's render_defaults(sun=)".into()));
    }
    let highlight = if req.highlight_selection {
        cadpole_engine::selection_from_proto(&doc.selection)
    } else {
        Vec::new()
    };
    let layers = cadpole_engine::layer_overrides_from_proto(&req.layers);
    let mut scene = cadpole_engine::scene_from_context(&mut ctx, &req.body_ids, &highlight, &layers);
    if let Some(sec) = &look.section {
        for w in cadpole_engine::section_scene(&mut scene, sec, ctx.tolerance) {
            diagnostics.push(pb::Diagnostic { severity: pb::Severity::Warning as i32, message: w, ..Default::default() });
        }
    }
    let cam_req = cadpole_engine::document_camera(&req.camera, ctx.render_defaults.as_ref());
    let camera = cadpole_engine::camera_for_scene(&scene, &cam_req, None, &opts);
    let model = ctx.to_model(true, ms);
    diagnostics.extend(model.diagnostics.iter().cloned());
    Ok(GpuPlan::Draw(GpuScene { scene, camera, opts, model, diagnostics }))
}

/// `cadpole render --backend gpu`: the wgpu renderer (docs/design.md) with no
/// window and no canvas, rendering to an offscreen texture and reading it back.
///
/// Returns `Ok(None)` when the machine has no adapter, having said so on
/// stderr; the caller then renders with the CPU rasterizer, which is the same
/// picture a little slower (the two are diffed in
/// `//src/rust/cadpole-render-wgpu:cadpole_render_wgpu_test`).
fn render_gpu(req: &pb::RenderRequest, stats: bool) -> anyhow::Result<Option<pb::RenderResponse>> {
    use cadpole_engine::cadpole_core::render::Item;

    // The debug views and the analysis views paint the surface per
    // triangle, which the wgpu renderer has no path for (its vertices carry
    // a face id, not a colour), and a debug view is a millisecond render on
    // the CPU anyway. Say so rather than silently drawing the plain part.
    if !req.debug.is_empty() || !req.analysis.is_empty() {
        let what = if req.debug.is_empty() { "--analysis" } else { "--debug" };
        eprintln!("note: {what} is drawn by the CPU rasterizer; --backend gpu ignored");
        return Ok(None);
    }
    // The wgpu shaders carry the studio rig as constants; the product rig
    // and the backdrops are the CPU's for now.
    if !req.lighting.is_empty() && req.lighting != "studio" || !req.backdrop.is_empty() {
        eprintln!("note: --lighting and --backdrop are drawn by the CPU rasterizer; --backend gpu ignored");
        return Ok(None);
    }
    // The adapter first: on a machine with none, the fallback renders on
    // the CPU without having evaluated the document here as well.
    let mut renderer = match cadpole_render_wgpu::Renderer::new(cadpole_render_wgpu::RendererOptions {
        picking: false,
        ..cadpole_render_wgpu::RendererOptions::new()
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: --backend gpu: {e}; falling back to the CPU rasterizer");
            return Ok(None);
        }
    };
    if stats {
        eprintln!("gpu adapter: {}", renderer.adapter());
    }
    let GpuScene { scene, camera, mut opts, model, diagnostics } = match gpu_scene(req)? {
        GpuPlan::Draw(g) => g,
        GpuPlan::Cpu(why) => {
            eprintln!("note: {why} is drawn by the CPU rasterizer; --backend gpu ignored");
            return Ok(None);
        }
    };
    // Sub-pixels are pixels of a larger render here too.
    let k = opts.supersample.clamp(1, 4);
    let (width, height) = (opts.width, opts.height);
    opts.supersample = 1;
    opts.width *= k;
    opts.height *= k;
    opts.edge_width_px *= k as f32;
    let items: Vec<Item<'_>> = scene
        .items
        .iter()
        .map(|i| Item {
            tri_colors: &i.tri_colors,
            paint: i.paint,
            mesh: &i.mesh,
            material: &i.material,
            face_frames: &i.face_frames,
            edges: &i.edges,
            edge_dihedrals: &i.edge_creases,
            highlight_faces: &i.highlight_faces,
            highlight_edges: &i.highlight_edges,
            opacity: None,
        })
        .collect();

    let t0 = cadpole_engine::cadpole_core::time::Stopwatch::start();
    let mut shot = |o: &RenderOptions| match renderer.render_with_stats(&items, &camera, o) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("warning: --backend gpu: {e}; falling back to the CPU rasterizer");
            None
        }
    };
    let (img, st) = if opts.transparent {
        // The GPU pipeline resolves onto an opaque target, so its alpha is
        // recovered from two renders, on black and on white: the difference
        // is exactly what each pixel's own samples left uncovered
        // (`alpha::difference_matte`). The ground shadow is drawn on the
        // background, so it becomes alpha when the catcher asks for it and
        // is left out otherwise.
        let flat = |bg: f32| RenderOptions { background: [bg; 3], contact_shadow: opts.contact_shadow && opts.shadow_catcher, ..opts.clone() };
        let Some((black, st)) = shot(&flat(0.0)) else { return Ok(None) };
        let Some((white, _)) = shot(&flat(1.0)) else { return Ok(None) };
        (cadpole_engine::cadpole_core::render::alpha::difference_matte(&black, &white), st)
    } else {
        let Some(v) = shot(&opts) else { return Ok(None) };
        v
    };
    let img = cadpole_engine::cadpole_core::render::alpha::downsample(&img, k);
    let total_ms = t0.elapsed_ms();
    let info = cadpole_engine::camera_info(&camera, &scene, width, height, 0, 0.0);
    Ok(Some(pb::RenderResponse {
        png: img.to_png(),
        width: img.width,
        height: img.height,
        diagnostics,
        model: Some(model),
        stats: Some(cadpole_engine::render_stats_to_proto(&st, total_ms)),
        camera: Some(info),
    }))
}

/// What `cadpole listen` does once the backlog is printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ListenMode {
    /// Print what is in the log and return.
    Backlog,
    /// Wait for the next printed event, then return.
    Next,
    /// Never return.
    Follow,
}

/// Which events `cadpole listen` prints.
#[derive(Clone, PartialEq, Eq, Debug)]
enum ListenFilter {
    /// The events worth starting a model turn for: a `message` the user
    /// typed, and a `selection_changed` that carries a note. Everything else
    /// (a bare selection, a parameter drag, a script edit) is context the
    /// reader can fetch when it next runs, and printing it would wake a
    /// watcher on every click.
    Wake,
    /// These kinds, whatever they carry.
    Kinds(Vec<String>),
    All,
}

impl ListenFilter {
    fn prints(&self, kind: &str, data: &serde_json::Value) -> bool {
        match self {
            ListenFilter::All => true,
            ListenFilter::Kinds(kinds) => kinds.iter().any(|k| k == kind),
            ListenFilter::Wake => match kind {
                "message" => data["from"].as_str() == Some("user"),
                "selection_changed" => data["note"].as_str().is_some_and(|n| !n.is_empty()),
                _ => false,
            },
        }
    }

    /// The `kinds` the server filters the long poll by: the poll is not
    /// released by traffic of any other kind. `assistant_listening` rides
    /// along when the Listen toggle is being followed.
    fn server_kinds(&self, gated: bool) -> Vec<String> {
        let mut kinds: Vec<String> = match self {
            ListenFilter::All => return Vec::new(),
            ListenFilter::Kinds(kinds) => kinds.clone(),
            ListenFilter::Wake => vec!["message".to_owned(), "selection_changed".to_owned()],
        };
        if gated && !kinds.iter().any(|k| k == "assistant_listening") {
            kinds.push("assistant_listening".to_owned());
        }
        kinds
    }
}

struct ListenOpts {
    mode: ListenMode,
    filter: ListenFilter,
    /// An explicit starting point; `None` resumes (see [`listen`]).
    after: Option<u64>,
    /// Print also while the UI's Listen toggle is off.
    always_listen: bool,
    /// The id the server keeps the cursor under.
    client: String,
    /// How long to wait before asking a server that stopped answering again.
    retry: std::time::Duration,
}

/// `cadpole listen`: session events, one JSON line each through `emit`.
///
/// `Backlog` prints what the log holds after `after` and returns. `Next` and
/// `Follow` are for a program that watches this command's output and is woken
/// by it, so they are silent unless there is something to act on:
///
/// * nothing is printed while nothing happens (no heartbeat, no banner);
/// * the UI's Listen toggle is honoured, as `cadpole mcp` honours it;
/// * the position is stored on the server under `client` after every batch,
///   so the next run continues after the last line this one printed and
///   what arrived in between is not lost; a client the server has never
///   seen starts at the end of the log, not at its beginning;
/// * a server that stops answering is asked again until it is back, and a
///   server that was restarted (a new session id, a log that starts over) is
///   read from the start of its log.
///
/// `connect` makes the client; it is called again after a failure because a
/// restarted server has a new token. `closed` says the reader has gone.
fn listen(
    connect: &dyn Fn() -> Box<dyn Host>,
    opts: &ListenOpts,
    emit: &mut dyn FnMut(String),
    note: &mut dyn FnMut(String),
    closed: &dyn Fn() -> bool,
) -> anyhow::Result<()> {
    let waits = opts.mode != ListenMode::Backlog;
    let gated = waits && !opts.always_listen;
    let mut host = connect();
    // (cursor, listening, session id). A failure here is the caller's: there
    // is no session to wait for yet.
    let attach = |host: &dyn Host| -> anyhow::Result<(u64, bool, String)> {
        if !waits {
            return Ok((opts.after.unwrap_or(0), true, String::new()));
        }
        let session = host.get_session(&Default::default())?;
        let cursor = match opts.after {
            Some(after) => after,
            None => {
                let held = host.ack(&pb::AckRequest { client: opts.client.clone(), cursor: 0 })?.cursor;
                if held > 0 {
                    held
                } else {
                    // New to this server: adopt the end of the log, and say
                    // so, or a run that prints nothing leaves 0 behind and
                    // the next one skips whatever arrived in between.
                    if session.last_event_seq > 0 {
                        host.ack(&pb::AckRequest { client: opts.client.clone(), cursor: session.last_event_seq })?;
                    }
                    session.last_event_seq
                }
            }
        };
        Ok((cursor, session.assistant_listening, session.id))
    };
    let (mut after, mut listening, mut session_id) = attach(host.as_ref())?;
    if gated && !listening {
        note("the UI's Listen toggle is off: waiting for it (--always-listen overrides it)".to_owned());
    }
    let mut lost = false;
    let mut printed = 0usize;
    loop {
        let req = pb::WaitEventsRequest {
            after,
            // 0 is not "do not wait": the server reads it as its 30 s
            // default (ops.proto). The backlog, when there is one, ends
            // the wait at once; 1 ms is what "no waiting" can be said as.
            timeout_ms: if waits { 30_000 } else { 1 },
            kinds: opts.filter.server_kinds(gated),
            client: if waits { opts.client.clone() } else { String::new() },
        };
        let resp: pb::WaitEventsResponse = match call("WaitEvents", &req, host.as_ref()) {
            Ok(resp) => resp,
            Err(e) if waits => {
                if !lost {
                    note(format!("the server stopped answering ({e:#}); waiting for it"));
                    lost = true;
                }
                if closed() {
                    return Ok(());
                }
                std::thread::sleep(opts.retry);
                host = connect();
                // A restarted server is a different session with a log that
                // starts over: this cursor means nothing there.
                if let Ok(session) = host.get_session(&Default::default()) {
                    if session.id != session_id {
                        note("the server was restarted: reading its log from the start".to_owned());
                        session_id = session.id;
                        after = 0;
                        listening = session.assistant_listening;
                    }
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        if lost {
            note("the server is answering again".to_owned());
            lost = false;
        }
        // The log is a window: say when this reader fell out of it, once,
        // rather than letting the gap look like quiet.
        if waits && resp.truncated && (listening || !gated) {
            emit(serde_json::json!({"kind": "events_truncated", "first_seq": resp.first_seq}).to_string());
            printed += 1;
        }
        // What `attach` read is the toggle as it is now, and the events in
        // front of the first change in this batch were sent under the toggle
        // as it was: the change says which. Without this a message typed
        // while Listen was off is printed by a run that starts after it was
        // turned on, and one typed while it was on is passed over by a run
        // that starts after it was turned off.
        if gated {
            if let Some(was) = resp.events.iter().find(|e| e.kind == "assistant_listening").and_then(listening_before) {
                listening = was;
            }
        }
        for e in &resp.events {
            let json = cadpole_ops::proto_to_json("cadpole.v1.Event", &e.encode_to_vec(), &cadpole_ops::serialize_options())?;
            if gated && e.kind == "assistant_listening" {
                listening = json["data"]["listening"].as_bool().unwrap_or(listening);
            }
            if gated && !listening {
                continue;
            }
            if opts.filter.prints(&e.kind, &json["data"]) {
                emit(json.to_string());
                printed += 1;
            }
        }
        if waits && resp.cursor > after {
            // Best effort: a failed ack costs a repeat, never an event.
            let _ = host.ack(&pb::AckRequest { client: opts.client.clone(), cursor: resp.cursor });
        }
        after = resp.cursor.max(after);
        let done = match opts.mode {
            ListenMode::Backlog => true,
            ListenMode::Next => printed > 0,
            ListenMode::Follow => false,
        };
        // Nobody reads a follow any more once stdout is closed.
        if done || closed() {
            return Ok(());
        }
    }
}

/// The Listen toggle as it stood before an `assistant_listening` event, when
/// the server said (`was`; a server that predates the field says nothing).
fn listening_before(e: &pb::Event) -> Option<bool> {
    let data = cadpole_ops::struct_to_json(e.data.as_ref()?);
    data["was"].as_bool()
}

/// The render flags `cadpole sweep` accepts (it shares `ViewArgs` with
/// `render`) but whose meaning `SweepRequest` has no field for: every cell
/// is a raster image with no section, analysis or debug view. Named here so
/// a sweep refuses them instead of dropping them.
fn sweep_unsupported(view: &ViewArgs) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut not = |set: bool, flag: &'static str| {
        if set {
            out.push(flag);
        }
    };
    not(view.section.is_some(), "--section");
    not(view.analysis.is_some(), "--analysis");
    not(view.min_wall.is_some(), "--min-wall");
    not(view.max_overhang.is_some(), "--max-overhang");
    not(view.debug.is_some(), "--debug");
    not(view.backend != "raster", "--backend");
    not(view.bounces.is_some(), "--bounces");
    not(view.dof.is_some(), "--dof");
    not(view.bloom, "--bloom");
    not(view.edge_break.is_some(), "--edge-break");
    not(view.denoise, "--denoise");
    not(view.haze.is_some(), "--haze");
    not(view.compare_edges, "--compare-edges");
    not(view.stats, "--stats");
    out
}

/// `cadpole sweep`'s request from its flags.
fn sweep_request(eval: &EvalArgs, view: &ViewArgs, tile: Option<&str>, cols: Option<u32>) -> anyhow::Result<pb::SweepRequest> {
    let unsupported = sweep_unsupported(view);
    if !unsupported.is_empty() {
        anyhow::bail!(
            "cadpole sweep does not take {}: every tile is a raster render of the whole model; use `cadpole render` \
             for one image with them",
            unsupported.join(", ")
        );
    }
    // Single-valued `--param` entries stay in the grid as fixed values;
    // cadpole-engine applies them to every cell.
    let (mut params, mut order) = (HashMap::new(), Vec::new());
    for p in &eval.params {
        let (k, v) = p.split_once('=').ok_or_else(|| anyhow::anyhow!("expected name=v1,v2,..., got {p:?}"))?;
        order.push(k.trim().to_owned());
        params.insert(
            k.trim().to_owned(),
            pb::ParamValues { values: v.split(',').map(|x| cadpole_engine::parse_value(x.trim()).to_proto()).collect() },
        );
    }
    // `--tile` and `--size` are the same knob here. Before, `--size`
    // was accepted and silently ignored, which meant a sweep quietly
    // came back at 320x240 however large you asked for.
    let size = parse_size(tile.or(view.size.as_deref()).unwrap_or("320x240"))?;
    Ok(pb::SweepRequest {
        params,
        param_order: order,
        width: size.0,
        height: size.1,
        camera: camera(view)?,
        cols: cols.unwrap_or(0),
        sun: view.sun.clone().unwrap_or_default(),
        north_degrees: view.north.unwrap_or(0.0),
        sun_color: view.sun_color.clone().unwrap_or_default(),
        sun_intensity: view.sun_intensity.unwrap_or(0.0),
        samples: view.samples.unwrap_or(0),
        threads: view.threads.unwrap_or(0),
        tolerance: eval.tolerance.unwrap_or(0.0),
        boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
        edge_mode: edge_mode(view.edges.as_deref()) as i32,
        grid_mode: grid_mode(view.grid.as_deref()) as i32,
        edge_crease_degrees: view.crease.unwrap_or(0.0).clamp(0.0, 180.0) as f64,
        edge_width_px: view.line_width.unwrap_or(0.0) as f64,
        xray: view.xray,
        transparent: view.background.as_deref().is_some_and(is_transparent),
        shadow_catcher: view.shadow_catcher,
        supersample: supersample(view)?,
        layers: layer_states(&view.layer)?,
        draft: view.draft,
        no_edges: view.no_edges,
        no_grid: view.no_grid,
        no_ambient_occlusion: view.no_ao,
        no_contact_shadow: view.no_shadow,
        no_silhouettes: view.no_silhouettes,
        exposure: exposure_arg(view)?,
        background: view.background.as_deref().filter(|b| !is_transparent(b)).map(background_color).transpose()?,
        show_environment: view.environment,
        lighting: view.lighting.clone().unwrap_or_default(),
        backdrop: view.backdrop.clone().unwrap_or_default(),
        floor: view.floor.clone().unwrap_or_default(),
        tonemap: view.tonemap.clone().unwrap_or_default(),
        ..Default::default()
    })
}

/// `cadpole render` on a document with errors: the image of a model that did
/// not build (empty, or the bodies made before the error) is not written,
/// as `export` and `animate` write nothing; the diagnostics above say why.
fn not_written(out: &std::path::Path) -> ExitCode {
    eprintln!("{} not written: the document has errors", out.display());
    ExitCode::from(1)
}

/// `cadpole check --bed X,Y,Z` (or `XxYxZ`, or one number for a cube).
fn bed_arg(bed: Option<&str>) -> anyhow::Result<Vec<f64>> {
    Ok(match bed {
        None | Some("") => Vec::new(),
        Some(spec) => spec
            .split(|c| c == ',' || c == 'x')
            .map(|n| match n.trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
                _ => Err(anyhow::anyhow!("--bed wants sizes in mm above 0, X,Y,Z or one number, got {spec:?}")),
            })
            .collect::<anyhow::Result<Vec<f64>>>()?,
    })
}

/// `cadpole check --bed-shape`.
fn bed_shape_arg(shape: Option<String>) -> anyhow::Result<String> {
    match shape {
        None => Ok(String::new()),
        Some(s) => match cadpole_engine::cadpole_print::BedShape::parse(s.trim()) {
            Some(b) => Ok(b.as_str().to_owned()),
            None => anyhow::bail!("--bed-shape {s:?}: rectangular or circular"),
        },
    }
}

/// `cadpole render -o` and `cadpole sweep -o`: the file is always a PNG.
fn png_out(out: &std::path::Path) -> anyhow::Result<()> {
    match out.extension().and_then(|e| e.to_str()) {
        None => Ok(()),
        Some(e) if e.eq_ignore_ascii_case("png") => Ok(()),
        Some(e) => anyhow::bail!("{}: this writes a PNG, not .{e}; name it .png", out.display()),
    }
}

/// `--layer NAME=...` names the render switched, checked against the
/// layers the evaluated document declares.
/// [`check_layer_names`] for a command whose own response carries no model
/// (a contact sheet, a sweep, an animation): one evaluation, skipped when no
/// `--layer` was given.
fn check_layers_by_evaluating(specs: &[String], eval: &EvalArgs, host: &dyn Host) -> anyhow::Result<()> {
    let states = layer_states(specs)?;
    if states.is_empty() {
        return Ok(());
    }
    let resp: pb::EvaluateResponse = call(
        "Evaluate",
        &pb::EvaluateRequest {
            tolerance: eval.tolerance.unwrap_or(0.0),
            boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
            skip_mesh: true,
            ..Default::default()
        },
        host,
    )?;
    check_layer_names(&states, resp.model.as_ref())
}

fn check_layer_names(states: &[pb::LayerState], model: Option<&pb::Model>) -> anyhow::Result<()> {
    let declared: Vec<&str> = model.map(|m| m.layers.iter().map(|l| l.name.as_str()).collect()).unwrap_or_default();
    for s in states {
        if s.name != "other" && !declared.contains(&s.name.as_str()) {
            anyhow::bail!(
                "--layer {:?}: the document declares no layer \"{}\"; its layers are {}",
                s.name,
                s.name,
                if declared.is_empty() { "none (only `other`, the bodies on no layer)".to_owned() } else { format!("{}, and `other`", declared.join(", ")) }
            );
        }
    }
    Ok(())
}

/// One line of `cadpole animate --list`.
fn animation_line(a: &pb::AnimationInfo) -> String {
    let times: Vec<String> = a.keyframe_times.iter().map(|t| format!("{}", (t * 1000.0).round() / 1000.0)).collect();
    format!(
        "{}: {} s at {} fps ({} frames{}), keyframes at {} s, moves {}{}{}",
        a.name,
        a.duration_s,
        a.fps,
        a.frames,
        if a.r#loop { ", loops" } else { "" },
        times.join(", "),
        if a.params.is_empty() { "nothing".to_owned() } else { a.params.join(", ") },
        if a.has_camera { " and the camera" } else { "" },
        if a.ease == "smooth" { String::new() } else { format!(", ease {}", a.ease) }
    )
}

/// `1.2 MB`, `340 kB`, `900 B`.
fn human_bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{} kB", n / 1000)
    } else {
        format!("{n} B")
    }
}

/// Render knobs from the CLI flags, as request fields.
fn render_request(eval: &EvalArgs, view: &ViewArgs, size: (u32, u32)) -> anyhow::Result<pb::RenderRequest> {
    Ok(pb::RenderRequest {
        width: size.0,
        height: size.1,
        camera: camera(view)?,
        sun: view.sun.clone().unwrap_or_default(),
        north_degrees: view.north.unwrap_or(0.0),
        sun_color: view.sun_color.clone().unwrap_or_default(),
        sun_intensity: view.sun_intensity.unwrap_or(0.0),
        haze: view.haze.clone().unwrap_or_default(),
        section: view.section.clone().unwrap_or_default(),
        samples: view.samples.unwrap_or(0),
        threads: view.threads.unwrap_or(0),
        backend: render_backend(&view.backend)? as i32,
        max_bounces: view.bounces.unwrap_or(0),
        draft: view.draft,
        no_edges: view.no_edges,
        no_grid: view.no_grid,
        no_ambient_occlusion: view.no_ao,
        no_contact_shadow: view.no_shadow,
        no_silhouettes: view.no_silhouettes,
        exposure: exposure_arg(view)?,
        background: view.background.as_deref().filter(|b| !is_transparent(b)).map(background_color).transpose()?,
        transparent: view.background.as_deref().is_some_and(is_transparent),
        shadow_catcher: view.shadow_catcher,
        supersample: supersample(view)?,
        show_environment: view.environment,
        lighting: view.lighting.clone().unwrap_or_default(),
        backdrop: view.backdrop.clone().unwrap_or_default(),
        floor: view.floor.clone().unwrap_or_default(),
        tonemap: view.tonemap.clone().unwrap_or_default(),
        dof_f_number: view.dof.unwrap_or(0.0),
        bloom: view.bloom,
        edge_break_mm: view.edge_break.unwrap_or(0.0),
        denoise: view.denoise,
        edge_mode: edge_mode(view.edges.as_deref()) as i32,
        grid_mode: grid_mode(view.grid.as_deref()) as i32,
        edge_crease_degrees: view.crease.unwrap_or(0.0).clamp(0.0, 180.0) as f64,
        edge_width_px: view.line_width.unwrap_or(0.0) as f64,
        xray: view.xray,
        layers: layer_states(&view.layer)?,
        analysis: view.analysis.clone().unwrap_or_default(),
        min_wall: view.min_wall.unwrap_or(0.0),
        max_overhang_degrees: view.max_overhang.unwrap_or(0.0),
        debug: view.debug.clone().unwrap_or_default(),
        tolerance: eval.tolerance.unwrap_or(0.0),
        boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
        param_overrides: override_map(&eval.params)?,
        ..Default::default()
    })
}

/// Print diagnostics to stderr; true when any of them is an error.
/// Do these two paths name the same file? Neither need exist yet, so this is
/// a lexical comparison after making both absolute, not `canonicalize`.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    let abs = |p: &std::path::Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    abs(a) == abs(b)
}

/// Where each of the script's own `export(...)` calls goes under `--out-dir`,
/// with the whole set worked out before any of it is written.
///
/// `--run-exports` keeps only the file name of each script path, which is what
/// stops a script writing outside `--out-dir`. Two script paths that differ
/// only in their directory therefore name the same file. Writing both would
/// print `wrote .../x.stl` twice and exit 0, leaving the second body and no
/// sign that the first had ever existed -- so it is an error, named on both
/// sides, before anything reaches the disk.
fn export_targets(
    paths: &[&str],
    out_dir: &std::path::Path,
    out: Option<&std::path::Path>,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for p in paths {
        let path = out_dir.join(std::path::Path::new(p).file_name().unwrap_or_default());
        if let Some(i) = targets.iter().position(|t| *t == path) {
            anyhow::bail!(
                "two exports would both be written to {}: {:?} and {p:?}. --run-exports keeps only \
                 the file name, so paths that differ only in their directory collide -- give them \
                 different file names.",
                path.display(),
                paths[i],
            );
        }
        if let Some(out) = out {
            if same_file(&path, out) {
                anyhow::bail!(
                    "the script's export {p:?} and --out would both be written to {}",
                    path.display(),
                );
            }
        }
        targets.push(path);
    }
    Ok(targets)
}

/// One line of `cadpole export`'s report per body: what the body
/// became in the file, at what tolerance and why that one, and how far the
/// triangles sit off the exact surface. The format is fixed: scripts read
/// it.
fn export_body_line(b: &pb::ExportBodyReport) -> String {
    if b.exact {
        return format!("{} {:?}: exact surfaces (STEP takes no tolerance)", b.body_id, b.name);
    }
    let made = if (b.tessellated_at - b.tolerance).abs() > 1e-12 {
        format!("; made at {:.4} mm", b.tessellated_at)
    } else {
        String::new()
    };
    let chord = match b.chord_error {
        Some(e) if !b.chord_error_face.is_empty() => format!("chord error {e:.4} mm on {}", b.chord_error_face),
        Some(e) => format!("chord error {e:.4} mm"),
        None => "chord error not measured".to_owned(),
    };
    format!(
        "{} {:?}: {} triangles at {} mm ({}{made}), {chord}",
        b.body_id, b.name, b.triangles, b.tolerance, b.tolerance_source
    )
}

/// Evaluate the document `cadpole bake`/`cadpole publish` names and wrap the model
/// as a bake, returning the message and its baked-model bytes. The
/// diagnostics are printed as `cadpole eval` prints them; an error stops here
/// unless `--allow-errors`, because a published page of half a model reads
/// as the model.
fn bake_document(eval: &EvalArgs, args: &BakeArgs) -> anyhow::Result<(pb::BakedModel, Vec<u8>)> {
    let doc = document(eval)?;
    let host = cadpole_ops::DocumentHost(doc.clone());
    let resp: pb::EvaluateResponse = call(
        "Evaluate",
        &pb::EvaluateRequest {
            tolerance: eval.tolerance.unwrap_or(0.0),
            boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
            ..Default::default()
        },
        &host,
    )?;
    let model = resp.model.unwrap_or_default();
    let failed = print_diagnostics(&model.diagnostics);
    anyhow::ensure!(!failed || args.allow_errors, "the script reported an error; nothing was baked (--allow-errors bakes it anyway)");
    let scale = args.model_unit.as_deref().map(publish::mm_per_unit).transpose()?.unwrap_or(1.0);
    let display_units = match (&args.units, &args.model_unit) {
        (Some(u), _) => Some(publish::display_units(u)?),
        // A script in feet or inches that declared no units of its own reads
        // in feet and inches, never in the millimetres it was scaled to.
        (None, Some(m)) if model.display_units.is_none() && matches!(publish::mm_per_unit(m)?, 25.4 | 304.8) => {
            Some(publish::display_units("ft-in")?)
        }
        _ => None,
    };
    let opts = publish::BakeOptions {
        title: args.title.clone(),
        include_sources: args.include_sources,
        // The meshes were made at this many of the script's units.
        tolerance: eval.tolerance.unwrap_or(0.05) * scale,
        scale,
        display_units,
        view: published_view(args, scale)?,
        hide_layers: args.hide_layer.clone(),
        show_params: args.show_params.as_deref().map(publish::ShowParams::parse).unwrap_or_default(),
    };
    publish::check_choices(&model, &opts)?;
    let baked = publish::bake(&doc, model, &opts);
    let bytes = publish::encode(&baked);
    Ok((baked, bytes))
}

fn body_count(b: &pb::BakedModel) -> usize {
    b.model.as_ref().map(|m| m.bodies.len()).unwrap_or(0)
}

fn size_label(n: usize) -> String {
    if n >= 1 << 20 { format!("{:.1} MB", n as f64 / (1 << 20) as f64) } else { format!("{:.1} KB", n as f64 / 1024.0) }
}

fn print_diagnostics(ds: &[pb::Diagnostic]) -> bool {
    let mut failed = false;
    for d in ds {
        let sev = match d.severity() {
            pb::Severity::Error => {
                failed = true;
                "error"
            }
            pb::Severity::Warning => "warning",
            _ => "info",
        };
        match &d.span {
            Some(s) => eprintln!("{sev}: {}:{}:{}: {}", s.path, s.start_line, s.start_col, d.message),
            None => eprintln!("{sev}: {}", d.message),
        }
    }
    failed
}

/// `cadpole inspect` without `--json`: one table per body, then the diagnostics
/// under the feature each belongs to. Everything is on stdout, the
/// diagnostics included, because here they are the report and not a side
/// channel (a script error is why there are no bodies).
fn print_inspection(resp: &pb::InspectResponse) {
    // The document's display convention: every length below is a
    // millimetre on the wire and is written here in the script's `units(...)`
    // — feet and inches for a house, millimetres by default. The one place
    // this report formats a number.
    let readout = Readout::from_proto(&resp.display_units);
    let at = |p: &Option<pb::Vec3>| p.as_ref().map(|p| readout.point(p)).unwrap_or_else(|| "-".to_owned());
    if let Some(u) = &resp.display_units {
        println!(
            "units {}{}",
            u.unit,
            if u.fraction > 0 { format!(" (dimensions to 1/{}\")", u.fraction) } else { String::new() }
        );
        println!();
    }
    // Instances first: which part each one places, where, and the
    // bodies below that are its posed copies.
    for i in &resp.instances {
        let pose = i.pose.as_ref();
        let t = at(&pose.and_then(|p| p.translation.clone()));
        let q = pose.and_then(|p| p.rotation.as_ref()).map(|q| describe_turn(q.x, q.y, q.z, q.w));
        let explode = i.explode.as_ref().filter(|e| e.x != 0.0 || e.y != 0.0 || e.z != 0.0);
        println!(
            "instance {}  part {:?}  at {t}  {}{}{}",
            i.id,
            i.part,
            q.unwrap_or_default(),
            if i.assembly.is_empty() { String::new() } else { format!("  in assembly {}", i.assembly) },
            explode.map(|e| format!("  exploded by ({:.3}, {:.3}, {:.3})", e.x, e.y, e.z)).unwrap_or_default(),
        );
        println!("  {:<10} {}", "bodies", i.body_ids.join(", "));
    }
    for b in &resp.bodies {
        println!("{}  {:?}  [{} {}]", b.body_id, b.name, b.feature_id, b.kind);
        println!(
            "  {:<10} {:>9}    {:<10} {:>9}    {:<10} {:>9}    {:<12} {:>7}",
            "faces", b.faces, "edges", b.edges, "triangles", b.triangles, "thin faces", b.thin_faces
        );
        println!(
            "  {:<10} {}",
            "surfaces",
            if b.surfaces.is_empty() {
                "none".to_owned()
            } else {
                b.surfaces.iter().map(|c| format!("{} {}", c.faces, c.kind)).collect::<Vec<_>>().join(", ")
            }
        );
        println!("  {:<10} {}", "bbox", readout.bbox(&b.bbox));
        println!("  {:<10} {}    {:<10} {}", "volume", readout.volume(b.volume_mm3), "area", readout.area(b.area_mm2));
        if let Some(s) = &b.solid {
            let shape = format!(
                "{} component{}, genus {}{}",
                s.components,
                if s.components == 1 { "" } else { "s" },
                if s.genus < 0 { "undefined".to_owned() } else { s.genus.to_string() },
                if s.pairs_skipped { ", crossings not checked" } else { "" }
            );
            if s.is_solid {
                println!("  {:<10} yes: {shape}", "solid");
            } else {
                println!("  {:<10} NO ({shape})", "solid");
                for f in &s.failing {
                    println!("    {:<14} {:>7}   first at {}   {}", f.check, f.count, at(&f.at), f.message);
                }
            }
            if s.degenerate > 0 {
                println!("  {:<10} {} triangle{} with no area", "degenerate", s.degenerate, if s.degenerate == 1 { "" } else { "s" });
            }
            match &s.worst_fold {
                Some(f) => println!(
                    "  {:<10} {}, worst at {}: tips {} apart, normals dot {:.5}, faces {} | {}",
                    "folds",
                    s.folds,
                    at(&f.at),
                    readout.gap(f.tip_gap_mm),
                    f.dot,
                    f.face_a,
                    f.face_b
                ),
                None => println!("  {:<10} none", "folds"),
            }
        }
        println!("  {:<10} {} at {}", "thinnest", readout.gap(b.min_altitude_mm), at(&b.min_altitude_at));
        println!(
            "  {:<10} {} at tolerance {} ({}{})",
            "chord error",
            match b.chord_error {
                Some(e) if !b.chord_error_face.is_empty() => {
                    format!("{} on {} at {}", readout.gap(e), b.chord_error_face, at(&b.chord_error_at))
                }
                Some(e) => format!("{} at {}", readout.gap(e), at(&b.chord_error_at)),
                None => "not measured".to_owned(),
            },
            readout.gap(b.tolerance),
            b.tolerance_source,
            if (b.tessellated_at - b.tolerance).abs() > 1e-12 {
                format!("; made at {}", readout.gap(b.tessellated_at))
            } else {
                String::new()
            }
        );
        println!(
            "  {:<10} {}",
            "manifold",
            match (b.manifold_verdict_known, b.manifold_self_intersecting) {
                (false, _) => "no verdict (it could not take the mesh)".to_owned(),
                (true, true) => "self-intersecting: its automatic engine choice would run the next boolean on the robust engine, losing face names".to_owned(),
                (true, false) => "not self-intersecting".to_owned(),
            }
        );
        println!();
    }
    if resp.bodies.is_empty() {
        println!("no bodies{}", if resp.ok { "" } else { " (the script did not evaluate; see below)" });
        println!();
    }
    let total: usize = resp.features.iter().map(|f| f.diagnostics.len()).sum();
    if total == 0 {
        println!("diagnostics: none");
        return;
    }
    println!("diagnostics ({total}):");
    for f in &resp.features {
        let head = if f.feature_id.is_empty() {
            "(document)".to_owned()
        } else {
            let span = f.span.as_ref().map(|s| format!("  {}:{}", s.path, s.start_line)).unwrap_or_default();
            format!("{} ({}){span}", f.feature_id, f.kind)
        };
        println!("  {head}");
        for d in &f.diagnostics {
            let sev = match d.severity() {
                pb::Severity::Error => "error",
                pb::Severity::Warning => "warning",
                _ => "info",
            };
            println!("    {sev}: {}", d.message);
        }
    }
}

fn value_label(v: &Option<pb::ParamValue>) -> String {
    v.as_ref().and_then(ParamValue::from_proto).map(|v| cadpole_engine::value_label(&v)).unwrap_or_default()
}

fn fmt_bbox(b: &Option<pb::BBox>) -> String {
    Readout { units: None }.bbox(b)
}

/// How `cadpole inspect` writes a number: in the document's `units(...)`
/// when the script declared one, else as millimetres, exactly as before.
struct Readout {
    units: Option<cadpole_engine::cadpole_drawing::units::Units>,
}

impl Readout {
    fn from_proto(u: &Option<pb::DisplayUnits>) -> Readout {
        Readout {
            units: u.as_ref().and_then(|u| cadpole_engine::cadpole_drawing::units::Units::parse(&u.unit, u.fraction).ok()),
        }
    }
    fn point(&self, p: &pb::Vec3) -> String {
        match &self.units {
            Some(u) => u.point([p.x, p.y, p.z]),
            None => format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z),
        }
    }
    fn bbox(&self, b: &Option<pb::BBox>) -> String {
        let v = |p: &Option<pb::Vec3>| {
            p.as_ref()
                .map(|p| match &self.units {
                    Some(u) => u.point([p.x, p.y, p.z]),
                    None => cadpole_engine::cadpole_core::context::fmt_vec(cadpole_engine::cadpole_core::Vec3::new(p.x, p.y, p.z)),
                })
                .unwrap_or_default()
        };
        b.as_ref().map(|b| format!("{} .. {}", v(&b.min), v(&b.max))).unwrap_or_default()
    }
    /// A gap, a thickness, a chord error: a decimal, never a fraction.
    fn gap(&self, mm: f64) -> String {
        match &self.units {
            Some(u) => u.clearance(mm),
            None => format!("{mm:.4} mm"),
        }
    }
    fn volume(&self, mm3: f64) -> String {
        match &self.units {
            Some(u) if u.unit.imperial() => format!("{:.1} cu ft", mm3 / (304.8 * 304.8 * 304.8)),
            Some(u) if u.unit == cadpole_engine::cadpole_drawing::LengthUnit::M => format!("{:.3} m³", mm3 / 1e9),
            _ => format!("{mm3:.3} mm³"),
        }
    }
    fn area(&self, mm2: f64) -> String {
        match &self.units {
            Some(u) if u.unit.imperial() || u.unit == cadpole_engine::cadpole_drawing::LengthUnit::M => u.area(mm2),
            _ => format!("{mm2:.3} mm²"),
        }
    }
}

/// Says on stderr, when a one-shot command ends, what its solves did: how
/// many answers came from the solution cache and how many were solved here.
/// A drop guard, so every return path of [`run`] says it.
struct OptSummary(bool);

impl Drop for OptSummary {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        if let Some(line) = opt_jobs::installed().and_then(|s| opt_jobs::summary(s.tally(), s.cache())) {
            eprintln!("{line}");
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    // The solver behind `opt.problem(...).solve()`: HiGHS, native
    // only, for every command this process runs -- `cadpole eval`, `cadpole test`,
    // `cadpole serve`'s evaluations and `cadpole mcp`'s alike, so one problem gets
    // one answer from all of them. The wasm engine and the Apple apps never
    // install one and say that solving needs native Cadpole. It is installed
    // behind the solution cache, so every one of those evaluations
    // reads the cache before it solves and files what it solves.
    opt_jobs::install();
    let cli = Cli::parse();
    let _summary = OptSummary(matches!(
        cli.cmd,
        Cmd::Eval { .. }
            | Cmd::Render { .. }
            | Cmd::Animate { .. }
            | Cmd::Sweep { .. }
            | Cmd::Export { .. }
            | Cmd::Check { .. }
            | Cmd::Plate { .. }
            | Cmd::Test { .. }
            | Cmd::Inspect { .. }
            | Cmd::Bake { .. }
            | Cmd::Publish { .. }
    ));
    // What `./assets` beside a script may hold, for every command that reads
    // one: the native caps, or the one `--max-asset-mb` sets.
    input::set_asset_limits(match cli.max_asset_mb {
        Some(mb) => cadpole_engine::cadpole_assets::Limits::with_max_asset_mb(mb),
        None => cadpole_engine::cadpole_assets::Limits::NATIVE,
    });
    // The heap a script may fill: 512 MB, or what `--max-heap-mb`
    // says, for every evaluation of this process.
    let _ = HEAP_BYTES.set(cli.max_heap_mb.map(heap_bytes_of).unwrap_or(cadpole_engine::Limits::DEFAULT_HEAP_BYTES));
    // A one-shot CLI run is not a server, but it is still not allowed to hang:
    // `cadpole eval` on a script with an infinite loop must stop and say so
    // (docs/security.md, "Starlark sandbox"). `--eval-budget-secs` moves the
    // budget; `cadpole serve` replaces it with its own, shorter one.
    cadpole_engine::limits::set_default_limits(cadpole_engine::Limits {
        max_heap_bytes: heap_bytes(),
        deadline_ms: eval_deadline_ms_of(cli.eval_budget_secs),
        ..cadpole_engine::Limits::batch()
    });
    match cli.cmd {
        Cmd::Eval { eval, json, model_out, jobs } => {
            match jobs.as_str() {
                "off" => {}
                "wait" => {
                    // `cadpole solve` first, quietly but for its jobs; then the
                    // evaluation below reads their answers from the cache.
                    let host = opt_jobs::ops::LocalHost::new(Some(document(&eval)?), local_job_settings("eval", None, None, None));
                    let r: pb::SolveResponse = call("Solve", &pb::SolveRequest { wait_ms: u32::MAX, ..Default::default() }, &host)?;
                    if !r.jobs.is_empty() {
                        eprintln!("solve jobs ({} evaluations):", r.evaluations);
                        for line in job_lines(&r.jobs) {
                            eprintln!("{line}");
                        }
                    }
                    if let Some(s) = opt_jobs::installed() {
                        s.reset_tally();
                    }
                }
                other => anyhow::bail!("--jobs {other:?}: off or wait"),
            }
            let host = cadpole_ops::DocumentHost(document(&eval)?);
            let name = host.0.name.clone();
            let resp: pb::EvaluateResponse = call(
                "Evaluate",
                &pb::EvaluateRequest {
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    ..Default::default()
                },
                &host,
            )?;
            let model = resp.model.unwrap_or_default();
            let failed = print_diagnostics(&model.diagnostics);
            if let Some(p) = model_out {
                std::fs::write(&p, model.encode_to_vec()).with_context(|| format!("writing {}", p.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&cadpole_engine::view::model_view_from_proto(&model))?);
            } else {
                println!("{} ({:.1} ms)", name, model.eval_millis);
                for p in &model.params {
                    let ov = if p.overridden { " (override)" } else { "" };
                    let unit = if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) };
                    println!("  param {} = {}{}{}", p.name, value_label(&p.value), unit, ov);
                }
                for f in &model.features {
                    let mut args: Vec<String> = f.args.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    args.sort();
                    println!("  {:<10} {:<16} {}", f.kind, f.id, args.join(" "));
                }
                for b in &model.bodies {
                    let t = b.topology.clone().unwrap_or_default();
                    let tris = b.mesh.as_ref().map(|m| m.indices.len() / 3).unwrap_or(0);
                    println!(
                        "  body {:<16} {} faces, {} edges, {} tris, bbox {}",
                        b.id,
                        t.faces.len(),
                        t.edges.len(),
                        tris,
                        fmt_bbox(&b.bbox)
                    );
                }
            }
            Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS })
        }
        Cmd::Bake { eval, out, bake } => {
            let (baked, bytes) = bake_document(&eval, &bake)?;
            std::fs::write(&out, &bytes).with_context(|| format!("writing {}", out.display()))?;
            eprintln!("wrote {} ({}, {} bodies, {})", out.display(), baked.name, body_count(&baked), size_label(bytes.len()));
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Publish { eval, out_dir, bake, force, web_dir } => {
            let web = server::web_dir(web_dir.clone()).ok_or_else(|| {
                anyhow::anyhow!("no web app to publish: this binary carries none; build //src/web:dist and pass --web-dir")
            })?;
            let (baked, bytes) = bake_document(&eval, &bake)?;
            let site = publish::publish(&web, &out_dir, &bytes, &baked, force)?;
            eprintln!(
                "published {} ({} bodies) to {}: {} ({}), {} files{}",
                baked.name,
                body_count(&baked),
                out_dir.display(),
                site.bake,
                size_label(site.bake_bytes),
                site.files.len(),
                if baked.document.is_some() { ", sources included" } else { "" }
            );
            println!("{}", site.index.display());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Render { eval, view, out, shadow_study, dates, times, at, camera_json } => {
            png_out(&out)?;
            let host = cadpole_ops::DocumentHost(input::load(&eval.input)?);
            let req = render_request(&eval, &view, parse_size(view.size.as_deref().unwrap_or("800x600"))?)?;
            if shadow_study || view.compare_edges {
                check_layers_by_evaluating(&view.layer, &eval, &host)?;
            }
            if shadow_study {
                let sun = view.sun.as_deref().unwrap_or_default();
                let Some(sheet) = shadow_study_sheet(&req, sun, dates.as_deref(), times.as_deref(), &at, &host)? else {
                    return Ok(not_written(&out));
                };
                std::fs::write(&out, sheet.to_png()).with_context(|| format!("writing {}", out.display()))?;
                eprintln!("wrote {} (shadow study, {}x{})", out.display(), sheet.width, sheet.height);
                return Ok(ExitCode::SUCCESS);
            }
            if view.compare_edges {
                let Some(sheet) = edge_variants(&req, &host)? else {
                    return Ok(not_written(&out));
                };
                std::fs::write(&out, sheet.to_png()).with_context(|| format!("writing {}", out.display()))?;
                eprintln!("wrote {} ({}x{})", out.display(), sheet.width, sheet.height);
                return Ok(ExitCode::SUCCESS);
            }
            // The document normally reaches an operation through the host;
            // the GPU path is local, so it is filled in here.
            let gpu = if wants_gpu(&view.backend) {
                let mut req = req.clone();
                req.document = Some(host.document()?);
                render_gpu(&req, view.stats)?
            } else {
                None
            };
            let resp: pb::RenderResponse = match gpu {
                Some(r) => r,
                None => call("Render", &req, &host)?,
            };
            let failed = print_diagnostics(&resp.diagnostics);
            if !failed {
                check_layer_names(&req.layers, resp.model.as_ref())?;
            }
            if view.stats {
                let s = resp.stats.unwrap_or_default();
                eprintln!(
                    "render {:.1} ms total ({} passes, {} threads): geometry {:.1} ssao {:.1} contact {:.1} shade {:.1} oit {:.1} overlay {:.1} tonemap+resolve {:.1}",
                    s.total_ms, s.passes, s.threads_used, s.geometry_ms, s.ssao_ms, s.contact_ms, s.shade_ms, s.oit_ms, s.overlay_ms, s.resolve_ms
                );
            }
            if failed {
                return Ok(not_written(&out));
            }
            std::fs::write(&out, &resp.png).with_context(|| format!("writing {}", out.display()))?;
            eprintln!("wrote {} ({}x{})", out.display(), resp.width, resp.height);
            if let Some(path) = &camera_json {
                write_camera_json(path, resp.camera.as_slice())?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Animate { eval, view, name, out, format, fps, frames, preview, list, variant, camera_json } => {
            let host = cadpole_ops::DocumentHost(input::load(&eval.input)?);
            // --format wins; otherwise the extension says what was meant, and
            // no extension means a directory of frames.
            let format = match format.map(|f| f.trim().trim_start_matches('.').to_ascii_lowercase()) {
                Some(f) => f,
                None => match out.as_ref().and_then(|o| o.extension()).and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
                    Some(e) if e == "gif" => "gif".to_owned(),
                    Some(e) if e == "apng" || e == "png" => "apng".to_owned(),
                    Some(other) => anyhow::bail!("unsupported animation format {other:?} (apng, gif, or a directory for frames)"),
                    None => if list { "apng".to_owned() } else { "frames".to_owned() },
                },
            };
            if out.is_none() && !list {
                anyhow::bail!("pass --out FILE (an .apng or .gif) or --out DIR --format frames, or --list");
            }
            if let Some(why) = animate_refuses(&view) {
                anyhow::bail!(why);
            }
            check_layers_by_evaluating(&view.layer, &eval, &host)?;
            // One render per --variant, or one at --size.
            if !variant.is_empty() && view.size.is_some() {
                anyhow::bail!("--variant names its own sizes; drop --size");
            }
            let runs: Vec<(Option<String>, (u32, u32))> = if variant.is_empty() {
                vec![(None, parse_size(view.size.as_deref().unwrap_or("800x600"))?)]
            } else {
                variant.iter().map(|v| parse_variant(v)).collect::<anyhow::Result<_>>()?
            };
            for (label, size) in runs {
                let render = render_request(&eval, &view, size)?;
                let req = pb::AnimateRequest {
                    name: name.clone().unwrap_or_default(),
                    list,
                    format: format.clone(),
                    width: render.width,
                    height: render.height,
                    camera: render.camera.clone(),
                    backend: render.backend,
                    samples: render.samples,
                    threads: render.threads,
                    draft: render.draft,
                    fps: fps.unwrap_or(0.0),
                    frames: frames.unwrap_or(0),
                    preview,
                    tolerance: render.tolerance,
                    boolean_tolerance: render.boolean_tolerance,
                    param_overrides: render.param_overrides.clone(),
                    no_edges: render.no_edges,
                    no_grid: render.no_grid,
                    no_ambient_occlusion: render.no_ambient_occlusion,
                    no_contact_shadow: render.no_contact_shadow,
                    no_silhouettes: render.no_silhouettes,
                    exposure: render.exposure,
                    background: render.background.clone(),
                    show_environment: render.show_environment,
                    lighting: render.lighting.clone(),
                    backdrop: render.backdrop.clone(),
                    floor: render.floor.clone(),
                    tonemap: render.tonemap.clone(),
                    dof_f_number: render.dof_f_number,
                    bloom: render.bloom,
                    edge_break_mm: render.edge_break_mm,
                    denoise: render.denoise,
                    max_bounces: render.max_bounces,
                    edge_mode: render.edge_mode,
                    grid_mode: render.grid_mode,
                    edge_crease_degrees: render.edge_crease_degrees,
                    edge_width_px: render.edge_width_px,
                    xray: render.xray,
                    layers: render.layers.clone(),
                    sun: render.sun.clone(),
                    north_degrees: render.north_degrees,
                    section: render.section.clone(),
                    transparent: render.transparent,
                    shadow_catcher: render.shadow_catcher,
                    supersample: render.supersample,
                    sun_color: render.sun_color.clone(),
                    sun_intensity: render.sun_intensity,
                    haze: render.haze.clone(),
                    ..Default::default()
                };
                let resp: pb::AnimateResponse = call("Animate", &req, &host)?;
                let failed = print_diagnostics(&resp.diagnostics);
                if list || failed {
                    if resp.animations.is_empty() {
                        println!("no animations: declare one with animation(name, keyframes) (docs/starlark-api.md)");
                    }
                    for a in &resp.animations {
                        println!("{}", animation_line(a));
                    }
                    return Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS });
                }
                let out = variant_path(out.as_ref().expect("checked above"), label.as_deref(), format == "frames");
                let t = resp.timings.clone().unwrap_or_default();
                if format == "frames" {
                    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
                    for f in &resp.frames {
                        let path = out.join(format!("frame_{:04}.png", f.index));
                        std::fs::write(&path, &f.png).with_context(|| format!("writing {}", path.display()))?;
                    }
                    eprintln!(
                        "wrote {} frames to {} ({}x{}, {:.1} s at {:.4} fps)",
                        resp.frames.len(),
                        out.display(),
                        resp.width,
                        resp.height,
                        resp.duration_s,
                        resp.fps
                    );
                } else {
                    std::fs::write(&out, &resp.data).with_context(|| format!("writing {}", out.display()))?;
                    eprintln!(
                        "wrote {} ({} {}, {} frames, {}x{}, {:.1} s at {:.4} fps, {})",
                        out.display(),
                        human_bytes(resp.bytes),
                        resp.format,
                        resp.frame_count,
                        resp.width,
                        resp.height,
                        resp.duration_s,
                        resp.fps,
                        if resp.r#loop { "looping" } else { "plays once" }
                    );
                }
                let later: Vec<&pb::AnimationFrameStat> = resp.frame_stats.iter().skip(1).collect();
                let n = later.len().max(1) as f64;
                let sum = |f: &dyn Fn(&pb::AnimationFrameStat) -> u32| later.iter().map(|s| f(s) as u64).sum::<u64>();
                eprintln!(
                    "time {:.1} s: first evaluation {:.2} s, then {:.0} ms per frame to evaluate, {:.0} ms per frame to render, camera fit {:.0} ms, encode {:.0} ms",
                    t.total_ms / 1000.0,
                    t.first_eval_ms / 1000.0,
                    t.later_eval_ms / resp.frame_stats.len().max(1) as f64,
                    t.render_ms / resp.frame_stats.len().max(1) as f64,
                    t.fit_ms,
                    t.encode_ms
                );
                eprintln!(
                    "after the first frame: {} kernel ops, {} tessellations, {:.1} bakes per frame; {} memo hits per frame",
                    sum(&|s| s.ops_run),
                    sum(&|s| s.tessellations),
                    sum(&|s| s.bakes) as f64 / n,
                    sum(&|s| s.ops_hit) as f64 / n
                );
                if let Some(path) = &camera_json {
                    write_camera_json(&variant_path(path, label.as_deref(), false), &resp.cameras)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Sweep { eval, view, out, tile, cols } => {
            png_out(&out)?;
            let req = sweep_request(&eval, &view, tile.as_deref(), cols)?;
            let host = cadpole_ops::DocumentHost(input::load(&eval.input)?);
            check_layers_by_evaluating(&view.layer, &eval, &host)?;
            let resp: pb::SweepResponse = call("Sweep", &req, &host)?;
            // The sheet's own warnings once (a param the document does not
            // declare is true of every cell), then each cell's own lines
            // under its label.
            let mut any_failed = print_diagnostics(&resp.diagnostics);
            for c in &resp.cells {
                let own: Vec<pb::Diagnostic> =
                    c.diagnostics.iter().filter(|d| !resp.diagnostics.iter().any(|s| s.message == d.message)).cloned().collect();
                if own.is_empty() {
                    continue;
                }
                eprintln!("tile {}:", c.label);
                if print_diagnostics(&own) {
                    any_failed = true;
                    eprintln!("  (tile {} failed)", c.label);
                }
            }
            std::fs::write(&out, &resp.png).with_context(|| format!("writing {}", out.display()))?;
            eprintln!("wrote {} ({} tiles, {}x{})", out.display(), resp.cells.len(), resp.width, resp.height);
            Ok(if any_failed { ExitCode::from(1) } else { ExitCode::SUCCESS })
        }
        Cmd::Export {
            eval,
            out,
            format,
            include_hidden,
            layers,
            bodies,
            model_unit,
            units,
            slicer,
            group,
            flavor,
            plate,
            printer,
            bed,
            template,
            plate_images,
            plate_image_size,
            plate_image_background,
            backend,
            run_exports,
            out_dir,
            view,
            section,
            look,
            title,
            subtitle,
            note,
            legend,
            labels,
            north,
            sheet,
            scale,
            crop,
        } => {
            // --format wins; otherwise the extension says what was meant.
            let named = format
                .map(|f| f.trim().trim_start_matches('.').to_ascii_lowercase())
                .or_else(|| {
                    out.as_ref()
                        .and_then(|o| o.extension())
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase())
                });
            if view.is_some() || section.is_some() {
                // One view as a drawing: the Drawing operation.
                let fmt = named.clone().unwrap_or_else(|| "pdf".to_owned());
                if !matches!(fmt.as_str(), "svg" | "pdf" | "dxf") {
                    anyhow::bail!("--view/--section draw a drawing: the format must be svg, pdf or dxf, not {fmt:?}");
                }
                let Some(out) = out else {
                    anyhow::bail!("--view/--section draw one page: pass --out FILE");
                };
                if run_exports {
                    anyhow::bail!("--view/--section draw one page; run --run-exports on its own");
                }
                let look = look.unwrap_or_else(|| "color".to_owned());
                let host = cadpole_ops::DocumentHost(document(&eval)?);
                let resp: pb::DrawingResponse = call(
                    "Drawing",
                    &pb::DrawingRequest {
                        view: view.clone().unwrap_or_default(),
                        format: fmt.clone(),
                        scale: scale.clone().unwrap_or_default(),
                        sheet: sheet.clone().unwrap_or_default(),
                        units: units.clone().unwrap_or_default(),
                        title: title.clone().unwrap_or_default(),
                        tolerance: eval.tolerance.unwrap_or(0.0),
                        look,
                        section: section.clone().unwrap_or_default(),
                        subtitle: subtitle.clone().unwrap_or_default(),
                        note: note.clone().unwrap_or_default(),
                        legend: legend.clone(),
                        labels: labels.clone(),
                        // One flag, read by what is being written: a
                        // drawing switches layers, a mesh selects by them.
                        layers: layer_states(&layers)?,
                        north_degrees: north.unwrap_or(0.0),
                        crop: crop.clone().unwrap_or_default(),
                        model_unit: model_unit.clone().unwrap_or_default(),
                        ..Default::default()
                    },
                    &host,
                )?;
                if print_diagnostics(&resp.diagnostics) {
                    return Ok(ExitCode::from(1));
                }
                for n in &resp.notes {
                    eprintln!("note: {n}");
                }
                std::fs::write(&out, &resp.data).with_context(|| format!("writing {}", out.display()))?;
                eprintln!("wrote {} ({}, scale {}, {} items)", out.display(), resp.format, resp.scale, resp.items);
                return Ok(ExitCode::SUCCESS);
            }
            let format = match named.as_deref() {
                Some("stl") | None => pb::ExportFormat::Stl,
                Some("obj") => pb::ExportFormat::Obj,
                Some("3mf") => pb::ExportFormat::Threemf,
                Some("glb") => pb::ExportFormat::Glb,
                Some("gltf") => pb::ExportFormat::Gltf,
                Some("step") | Some("stp") => pb::ExportFormat::Step,
                Some("svg") => pb::ExportFormat::Svg,
                Some("pdf") => pb::ExportFormat::Pdf,
                Some("dxf") => pb::ExportFormat::Dxf,
                Some(other) => anyhow::bail!(
                    "unsupported export format {other:?} (stl, obj, 3mf, glb, gltf, step, svg, pdf, dxf)"
                ),
            };
            if out.is_none() && !run_exports {
                anyhow::bail!("nothing to export: pass --out FILE and/or --run-exports");
            }
            let plate_image_size = plate_image_size.as_deref().map(parse_size).transpose()?.unwrap_or((0, 0));
            let plate_image_backend = match backend.trim() {
                "raster" | "" => pb::RenderBackend::Raster,
                "path" => pb::RenderBackend::PathTrace,
                other => anyhow::bail!("--backend must be raster or path for plate images, got {other:?}"),
            };
            let mut doc = document(&eval)?;
            let bed: Vec<f64> = match bed.as_deref() {
                None => Vec::new(),
                Some(s) => s
                    .split([',', 'x'])
                    .map(|v| v.trim().parse::<f64>().map_err(|_| anyhow::anyhow!("--bed takes X,Y or X,Y,Z in mm, got {s:?}")))
                    .collect::<anyhow::Result<_>>()?,
            };
            let template = match template.as_deref() {
                None => String::new(),
                Some(t) => template_asset(&mut doc, t)?,
            };
            let host = cadpole_ops::DocumentHost(doc);
            let resp: pb::ExportResponse = call(
                "Export",
                &pb::ExportRequest {
                    format: format as i32,
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    run_script_exports: run_exports,
                    bed,
                    template,
                    // Without --out nothing reads the whole-document export,
                    // so it is neither made nor judged: a drawing set's
                    // export meshes and checks only what it draws.
                    script_exports_only: out.is_none(),
                    slicer: slicer.clone(),
                    group: group.clone(),
                    flavor: flavor.clone(),
                    plate,
                    printer: printer.clone().unwrap_or_default(),
                    plate_images: plate_images.is_some(),
                    plate_image_width: plate_image_size.0,
                    plate_image_height: plate_image_size.1,
                    plate_image_background: plate_image_background.clone().unwrap_or_default(),
                    plate_image_backend: plate_image_backend as i32,
                    include_hidden_layers: include_hidden,
                    model_unit: model_unit.clone().unwrap_or_default(),
                    units: units.clone().unwrap_or_default(),
                    body_ids: bodies.clone(),
                    layer_names: layer_names_arg(&layers)?,
                    ..Default::default()
                },
                &host,
            )?;
            if print_diagnostics(&resp.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            let paths: Vec<&str> = resp.files.iter().map(|f| f.path.as_str()).collect();
            let targets = export_targets(&paths, &out_dir, out.as_deref())?;
            let file_name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // The plate images go under --plate-images when it was given,
            // else beside the project they belong to.
            let image_dir = |beside: &std::path::Path| {
                plate_images.clone().unwrap_or_else(|| beside.parent().map(|p| p.to_path_buf()).unwrap_or_default())
            };
            let write_images = |dir: &std::path::Path, stem: &str, images: &[pb::PlateImage], sheet: &[u8]| -> anyhow::Result<()> {
                if images.is_empty() {
                    return Ok(());
                }
                std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
                for img in images {
                    let path = dir.join(&img.path);
                    std::fs::write(&path, &img.png).with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("wrote {} ({}x{}, plate {})", path.display(), img.width, img.height, img.plate);
                }
                if !sheet.is_empty() {
                    let path = dir.join(format!("{stem}_plates.png"));
                    std::fs::write(&path, sheet).with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("wrote {} (contact sheet, {} plates)", path.display(), images.len());
                }
                Ok(())
            };
            let stem_of = |p: &std::path::Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if resp.files.is_empty() && out.is_none() {
                eprintln!("nothing written: the script makes no export() calls (pass --out FILE for the whole document)");
            }
            for (path, f) in targets.iter().zip(&resp.files) {
                std::fs::write(path, &f.data)?;
                eprintln!("wrote {}", path.display());
                for b in &f.bodies {
                    println!("{}  {}", file_name(path), export_body_line(b));
                }
                write_images(&image_dir(path), &stem_of(path), &f.plate_images, &f.plate_sheet)?;
            }
            if let Some(out) = out {
                std::fs::write(&out, &resp.data)?;
                eprintln!("wrote {}", out.display());
                for b in &resp.bodies {
                    println!("{}  {}", file_name(&out), export_body_line(b));
                }
                // The engine names the images after the document, as it
                // names `suggested_filename`, whatever --out is called.
                let stem = stem_of(std::path::Path::new(&resp.suggested_filename));
                write_images(&image_dir(&out), &stem, &resp.plate_images, &resp.plate_sheet)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Plate {
            eval,
            out,
            printer,
            ams,
            variants,
            spacing,
            joint_gap,
            max_bridge,
            layers,
            coupon_layers,
            bodies,
            include_hidden,
            script_objects,
            template,
            plate_images,
            json,
        } => {
            let mut doc = document(&eval)?;
            let template = match template.as_deref() {
                None => String::new(),
                Some(t) => template_asset(&mut doc, t)?,
            };
            let title = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let host = cadpole_ops::DocumentHost(doc);
            let resp: pb::PlateResponse = call(
                "Plate",
                &pb::PlateRequest {
                    body_ids: bodies,
                    layer_names: layer_names_arg(&layers)?,
                    include_hidden_layers: include_hidden,
                    printer: printer.unwrap_or_default(),
                    ams: ams.unwrap_or(0),
                    variants,
                    spacing: spacing.unwrap_or(0.0),
                    joint_gap: joint_gap.unwrap_or(0.0),
                    max_bridge: match max_bridge {
                        Some(m) if m <= 0.0 => -1.0,
                        Some(m) => m,
                        None => 0.0,
                    },
                    coupon_layers,
                    script_objects,
                    template,
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    plate_images: plate_images.is_some(),
                    title: title.clone(),
                    ..Default::default()
                },
                &host,
            )?;
            if print_diagnostics(&resp.diagnostics) || resp.data.is_empty() {
                return Ok(ExitCode::from(1));
            }
            std::fs::write(&out, &resp.data).with_context(|| format!("writing {}", out.display()))?;
            eprintln!("wrote {}", out.display());
            if let Some(dir) = plate_images {
                std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
                for img in &resp.plate_images {
                    let path = dir.join(&img.path);
                    std::fs::write(&path, &img.png).with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("wrote {} ({}x{}, plate {})", path.display(), img.width, img.height, img.plate);
                }
                if !resp.plate_sheet.is_empty() {
                    let path = dir.join(format!("{title}_plates.png"));
                    std::fs::write(&path, &resp.plate_sheet).with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("wrote {}", path.display());
                }
            }
            if json {
                let mut slim = resp.clone();
                slim.data.clear();
                slim.plate_sheet.clear();
                slim.plate_images.clear();
                let v = cadpole_ops::proto_to_json("cadpole.v1.PlateResponse", &slim.encode_to_vec(), &cadpole_ops::serialize_options())?;
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!("{}", resp.report);
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Check {
            eval,
            printer,
            min_wall,
            nozzle,
            perimeters,
            layer_height,
            material,
            max_overhang,
            bed,
            bed_shape,
            orient,
            max_bridge,
            max_overhang_area,
            max_cantilever,
            max_horizontal_hole,
            min_island,
            max_slenderness,
            slender_width,
            max_height_to_base,
            support_free,
            orientation,
            orientation_tolerance,
            layers,
            bodies,
            include_hidden,
            json,
        } => {
            // 0 in the request is "the profile's"; a flag's own 0 is the
            // strictest rule (or, for a ratio, the check off), which the
            // request spells as a negative number.
            let rule = |v: Option<f64>| match v {
                Some(m) if m <= 0.0 => -1.0,
                Some(m) => m,
                None => 0.0,
            };
            let bed = bed_arg(bed.as_deref())?;
            let bed_shape = bed_shape_arg(bed_shape)?;
            let host = cadpole_ops::DocumentHost(document(&eval)?);
            let resp: pb::PrintCheckResponse = call(
                "PrintCheck",
                &pb::PrintCheckRequest {
                    printer: printer.unwrap_or_default(),
                    min_wall: min_wall.unwrap_or(0.0),
                    nozzle: nozzle.unwrap_or(0.0),
                    perimeters: perimeters.unwrap_or(0.0),
                    layer_height: layer_height.unwrap_or(0.0),
                    material: material.unwrap_or_default(),
                    max_overhang_degrees: max_overhang.unwrap_or(0.0),
                    bed,
                    bed_shape,
                    orient,
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    body_ids: bodies,
                    layer_names: layer_names_arg(&layers)?,
                    include_hidden_layers: include_hidden,
                    // 0 is "the profile's"; the flag's own 0 means no bridge
                    // at all, which the request spells as a negative number.
                    max_bridge: match max_bridge {
                        Some(m) if m <= 0.0 => -1.0,
                        Some(m) => m,
                        None => 0.0,
                    },
                    max_overhang_area: rule(max_overhang_area),
                    max_cantilever: rule(max_cantilever),
                    max_horizontal_hole: rule(max_horizontal_hole),
                    min_island: rule(min_island),
                    max_slenderness: rule(max_slenderness),
                    slender_width: slender_width.unwrap_or(0.0),
                    max_height_to_base: rule(max_height_to_base),
                    support_free,
                    optimal_orientation: orientation,
                    orientation_tolerance: orientation_tolerance.unwrap_or(0.0),
                    ..Default::default()
                },
                &host,
            )?;
            if print_diagnostics(&resp.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            if json {
                let v = cadpole_ops::proto_to_json(
                    "cadpole.v1.PrintCheckResponse",
                    &resp.encode_to_vec(),
                    &cadpole_ops::serialize_options(),
                )?;
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(if resp.ok { ExitCode::SUCCESS } else { ExitCode::from(1) });
            }
            for b in &resp.bodies {
                println!("{}  {}", b.name, b.summary);
                if let Some(o) = &b.orientation {
                    println!("  orientation: {}", o.reasoning);
                }
                for i in &b.issues {
                    // Most messages already carry their position; only add one
                    // when the text does not.
                    let at = match &i.at {
                        Some(p) if !i.message.contains(" at (") => {
                            format!(" at ({:.2}, {:.2}, {:.2})", p.x, p.y, p.z)
                        }
                        _ => String::new(),
                    };
                    println!("  {:<7} [{}] {}{}", i.severity, i.check, i.message, at);
                }
                if b.issues.is_empty() {
                    println!("  {}", if support_free { "prints without support" } else { "nothing to report" });
                }
            }
            Ok(if resp.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Test { eval, name, threads, json } => {
            let host = cadpole_ops::DocumentHost(document(&eval)?);
            let resp: pb::RunTestsResponse = call(
                "RunTests",
                &pb::RunTestsRequest {
                    names: name,
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    threads: threads.unwrap_or(0),
                    ..Default::default()
                },
                &host,
            )?;
            let built = !print_diagnostics(&resp.diagnostics);
            if json {
                let v = cadpole_ops::proto_to_json(
                    "cadpole.v1.RunTestsResponse",
                    &resp.encode_to_vec(),
                    &cadpole_ops::serialize_options(),
                )?;
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(if resp.ok { ExitCode::SUCCESS } else { ExitCode::from(1) });
            }
            if !built {
                eprintln!("the model did not build, so no test ran");
                return Ok(ExitCode::from(1));
            }
            // One line per result, the sweep point and the returned number
            // after the name; a failure adds its message and where it was
            // raised. A pass says nothing but its name. The number is for
            // reading across a sweep, so three decimals; `--json` keeps it
            // whole.
            for r in &resp.results {
                let point = if r.label.is_empty() { String::new() } else { format!("  {}", r.label) };
                let value = r.value.map(|v| format!("  = {}", (v * 1000.0).round() / 1000.0)).unwrap_or_default();
                if r.ok {
                    println!("PASS {}{point}{value}", r.name);
                } else {
                    println!("FAIL {}{point}: {}", r.name, r.message);
                    if let Some(s) = &r.span {
                        println!("     {}:{}:{}", s.path, s.start_line, s.start_col);
                    }
                }
            }
            if resp.declared == 0 {
                println!("no tests: declare one with test(name, fn) (docs/starlark-api.md)");
            } else {
                println!("{} passed, {} failed", resp.passed, resp.failed);
            }
            Ok(if resp.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Inspect { eval, bodies, json } => {
            let host = cadpole_ops::DocumentHost(document(&eval)?);
            let resp: pb::InspectResponse = call(
                "Inspect",
                &pb::InspectRequest {
                    body_ids: bodies,
                    tolerance: eval.tolerance.unwrap_or(0.0),
                    boolean_tolerance: eval.boolean_tolerance.unwrap_or(0.0),
                    ..Default::default()
                },
                &host,
            )?;
            if json {
                let v = cadpole_ops::proto_to_json(
                    "cadpole.v1.InspectResponse",
                    &resp.encode_to_vec(),
                    &cadpole_ops::serialize_options(),
                )?;
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print_inspection(&resp);
            }
            Ok(if resp.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Doc { json } => {
            let doc: pb::ApiDocResponse = call("ApiDoc", &pb::ApiDocRequest {}, &cadpole_ops::NoHost)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&doc_json(&doc))?);
            } else {
                print!("{}", doc_markdown(&doc));
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Serve(args) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(server::serve(args))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Listen { url, token, after, next, follow, kinds, all, always_listen, client } => {
            let mode = if next {
                ListenMode::Next
            } else if follow {
                ListenMode::Follow
            } else {
                ListenMode::Backlog
            };
            let filter = if all || (kinds.is_empty() && mode == ListenMode::Backlog) {
                ListenFilter::All
            } else if kinds.is_empty() {
                ListenFilter::Wake
            } else {
                ListenFilter::Kinds(kinds)
            };
            let opts = ListenOpts { mode, filter, after, always_listen, client, retry: std::time::Duration::from_secs(2) };
            // One endpoint for every connection: it reads the token afresh
            // and, with no --url, finds the server again after a restart --
            // the same document's, never another one's.
            let tokens = match token.clone().filter(|t| !t.is_empty()) {
                Some(t) => runtime::TokenSource::Explicit(t),
                None => runtime::TokenSource::Owner { file: None },
            };
            let endpoint = std::sync::Arc::new(runtime::Endpoint::new(url.as_deref(), tokens)?);
            let where_ = url.clone().unwrap_or_else(|| "the runtime directory".to_owned());
            let connect = || -> Box<dyn Host> {
                endpoint.invalidate();
                let client = if opts.mode == ListenMode::Backlog { format!("cli:{}", std::process::id()) } else { opts.client.clone() };
                Box::new(ops_host::HttpHost::on(endpoint.clone(), client))
            };
            // Events on stdout, one line each; everything else on stderr, so
            // a watcher of the output is woken by events only.
            listen(&connect, &opts, &mut |line| println!("{line}"), &mut |line| eprintln!("cadpole listen: {line}"), &stdout_closed)
                .with_context(|| format!("cadpole listen: no session at {where_} (is `cadpole serve` running?)"))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Selection { url, token, json } => {
            let tokens = match token.filter(|t| !t.is_empty()) {
                Some(t) => runtime::TokenSource::Explicit(t),
                None => runtime::TokenSource::Owner { file: None },
            };
            let host = ops_host::HttpHost::on(std::sync::Arc::new(runtime::Endpoint::new(url.as_deref(), tokens)?), format!("cli:{}", std::process::id()));
            let resp: pb::GetSelectionResponse = call("GetSelection", &pb::GetSelectionRequest::default(), &host)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&cadpole_ops::proto_to_json(
                        "cadpole.v1.GetSelectionResponse",
                        &resp.encode_to_vec(),
                        &cadpole_ops::serialize_options()
                    )?)?
                );
            } else {
                if resp.refs.is_empty() {
                    println!("nothing selected");
                }
                for r in &resp.refs {
                    let e = r.r#ref.clone().unwrap_or_default();
                    let status = match r.status() {
                        pb::RefStatus::Ok | pb::RefStatus::Unspecified => String::new(),
                        other => format!(" [{}: {}]", cadpole_ops::snake_case(other.as_str_name().trim_start_matches("REF_STATUS_")), r.reason),
                    };
                    println!(
                        "{} {}{}{}{}",
                        cadpole_engine::kind_name(e.kind()),
                        e.feature_id,
                        if e.body_id.is_empty() { String::new() } else { format!(" body={}", e.body_id) },
                        if e.topo_name.is_empty() { String::new() } else { format!(" {}", e.topo_name) },
                        status
                    );
                    if let Some(f) = &r.face {
                        let c = f.centroid.clone().unwrap_or_default();
                        let n = f.normal.clone().unwrap_or_default();
                        println!(
                            "    {} face, area {:.1} mm2, centroid ({:.1}, {:.1}, {:.1}), normal ({:.2}, {:.2}, {:.2})",
                            f.surface_kind, f.area, c.x, c.y, c.z, n.x, n.y, n.z
                        );
                    }
                    if let Some(g) = &r.edge {
                        println!("    {} edge, length {:.1} mm", g.curve_kind, g.length);
                    }
                    if let Some(f) = &r.feature {
                        let at = f
                            .span
                            .as_ref()
                            .map(|s| format!(" at {}:{}:{}", s.path, s.start_line, s.start_col))
                            .unwrap_or_default();
                        println!("    from {} {}{}", f.kind, f.id, at);
                    }
                }
                if !resp.note.is_empty() {
                    println!("note: {}", resp.note);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Op { args } => op_cli::run(&args),
        Cmd::OptJob { root, id } => Ok(opt_jobs::child::run(&root, &id)),
        Cmd::Solve { eval, jobs, time_limit, mem_mb, timeout, detach, rerun, json } => {
            let host = opt_jobs::ops::LocalHost::new(Some(document(&eval)?), local_job_settings("solve", jobs, time_limit, mem_mb));
            let wait_ms = match (detach, timeout) {
                (true, _) => 0,
                (false, Some(t)) => (t.max(0.0) * 1000.0).min(u32::MAX as f64 - 1.0) as u32,
                (false, None) => u32::MAX,
            };
            let r: pb::SolveResponse = call("Solve", &pb::SolveRequest { wait_ms, rerun, ..Default::default() }, &host)?;
            if json {
                let v = cadpole_ops::proto_to_json("cadpole.v1.SolveResponse", &r.encode_to_vec(), &cadpole_ops::serialize_options())?;
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print_diagnostics(&r.diagnostics);
                print_jobs(&r.jobs);
                println!(
                    "solve: {} job{}, {} pending, {} evaluation{}{}",
                    r.jobs.len(),
                    if r.jobs.len() == 1 { "" } else { "s" },
                    r.pending,
                    r.evaluations,
                    if r.evaluations == 1 { "" } else { "s" },
                    if r.ok {
                        "; done"
                    } else if detach {
                        "; detached: `cadpole jobs` follows them"
                    } else {
                        ""
                    }
                );
            }
            Ok(if r.ok || (detach && r.jobs.iter().all(|j| j.state != "failed" && j.state != "cancelled")) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Cmd::Jobs { action, all, json } => {
            let host = opt_jobs::ops::LocalHost::new(None, local_job_settings("jobs", None, None, None));
            let action = action.unwrap_or(JobsCmd::List { ids: Vec::new(), all, json });
            let (jobs, json, cache, ok) = match action {
                JobsCmd::List { ids, all, json } => {
                    let r: pb::ListJobsResponse = call("ListJobs", &pb::ListJobsRequest { ids, all }, &host)?;
                    (r.jobs, json, r.cache, true)
                }
                JobsCmd::Wait { ids, timeout, json } => {
                    let timeout_ms = timeout.map(|t| (t.max(0.001) * 1000.0).min(u32::MAX as f64 - 1.0) as u32).unwrap_or(u32::MAX);
                    let r: pb::WaitJobsResponse = call("WaitJobs", &pb::WaitJobsRequest { ids, timeout_ms }, &host)?;
                    (r.jobs, json, None, r.done)
                }
                JobsCmd::Cancel { ids, all, forget, json } => {
                    let r: pb::CancelJobsResponse = call("CancelJobs", &pb::CancelJobsRequest { ids, all, forget }, &host)?;
                    (r.jobs, json, None, true)
                }
            };
            if json {
                let v = serde_json::json!({
                    "jobs": jobs.iter().map(|j| cadpole_ops::proto_to_json("cadpole.v1.SolveJob", &j.encode_to_vec(), &cadpole_ops::serialize_options())).collect::<anyhow::Result<Vec<_>>>()?,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                if jobs.is_empty() {
                    println!("no solve jobs");
                }
                print_jobs(&jobs);
                if let Some(c) = cache {
                    println!(
                        "cache: {} answer{}, {:.1} MB of {} MB, in {}",
                        c.answers,
                        if c.answers == 1 { "" } else { "s" },
                        c.bytes as f64 / 1048576.0,
                        c.cap_bytes / 1048576,
                        c.path
                    );
                }
            }
            Ok(if ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Mcp(args) => {
            mcp::run(args)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub fn doc_json(doc: &pb::ApiDocResponse) -> serde_json::Value {
    let f = |f: &pb::ApiFunction| {
        serde_json::json!({
            "name": f.name, "doc": f.doc, "returns": f.returns, "on_type": f.on_type, "constant": f.constant,
            "params": f.params.iter().map(|p| serde_json::json!({
                "name": p.name, "type": p.r#type, "doc": p.doc, "required": p.required, "default_value": p.default_value
            })).collect::<Vec<_>>()
        })
    };
    serde_json::json!({
        "functions": doc.functions.iter().map(f).collect::<Vec<_>>(),
        "types": doc.types.iter().map(|t| serde_json::json!({
            "name": t.name, "doc": t.doc,
            "methods": t.methods.iter().map(f).collect::<Vec<_>>(),
            "attrs": t.attrs.iter().map(|p| serde_json::json!({"name": p.name, "type": p.r#type, "doc": p.doc})).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

pub fn doc_markdown(doc: &pb::ApiDocResponse) -> String {
    use std::fmt::Write;
    let mut s = String::from("# Cadpole Starlark API (generated by `cadpole doc`)\n\n## Functions\n\n");
    let sig = |f: &pb::ApiFunction| {
        if f.constant {
            return format!("{}: {}", f.name, f.returns);
        }
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                if !p.default_value.is_empty() {
                    format!("{}={}", p.name, p.default_value)
                } else if p.required {
                    p.name.clone()
                } else {
                    format!("{}=None", p.name)
                }
            })
            .collect();
        format!("{}({}) -> {}", f.name, params.join(", "), f.returns)
    };
    for f in &doc.functions {
        let _ = writeln!(s, "### `{}`\n\n{}\n", sig(f), f.doc);
    }
    s.push_str("## Types\n\n");
    for t in &doc.types {
        let _ = writeln!(s, "### {}\n", t.name);
        if !t.doc.is_empty() {
            let _ = writeln!(s, "{}\n", t.doc);
        }
        // A vocabulary is all attributes and no methods, so without
        // this line `kind` would print as a heading with nothing under it.
        for a in &t.attrs {
            let _ = writeln!(s, "- `{}.{}`: {}", t.name, a.name, a.doc.lines().next().unwrap_or(""));
        }
        for m in &t.methods {
            let _ = writeln!(s, "- `{}.{}`: {}", t.name, sig(m), m.doc.lines().next().unwrap_or(""));
        }
        s.push('\n');
    }
    s
}

/// Where `bazel run` was typed, when this process is the one it started.
///
/// `bazel run` starts the binary with its runfiles tree (`<binary>.runfiles/...`)
/// as the working directory and names the directory the command was typed in
/// `BUILD_WORKING_DIRECTORY`. Relative paths on the command line are meant
/// against the latter. The variable alone is not enough: it is inherited by
/// everything a `bazel run` target starts, and a `cadpole` launched by such a
/// tool from its own directory must keep that directory. So the working
/// directory has to be inside this binary's runfiles tree as well. `binaries`
/// are the paths this binary may be known by (the resolved executable, and
/// `argv[0]` when it is absolute); the returned runfiles directory keeps
/// runfiles lookups working once the process has moved out of it.
fn bazel_run_directories(build_working_directory: Option<&std::ffi::OsStr>, cwd: &std::path::Path, binaries: &[std::path::PathBuf]) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let wd = std::path::PathBuf::from(build_working_directory.filter(|d| !d.is_empty())?);
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    binaries.iter().find_map(|bin| {
        let mut tree = bin.clone().into_os_string();
        tree.push(".runfiles");
        let tree = std::path::PathBuf::from(tree);
        let tree = std::fs::canonicalize(&tree).unwrap_or(tree);
        cwd.starts_with(&tree).then(|| (wd.clone(), tree))
    })
}

/// Under `bazel run`, move to the directory the command was typed in, so
/// `bazel run //:cadpole -- render examples/bracket.star -o b.png` reads and
/// writes where a built binary would.
fn enter_bazel_run_working_directory() {
    let Ok(cwd) = std::env::current_dir() else { return };
    let mut binaries = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        binaries.push(exe);
    }
    if let Some(arg0) = std::env::args_os().next().map(std::path::PathBuf::from).filter(|p| p.is_absolute()) {
        binaries.push(arg0);
    }
    let Some((wd, runfiles)) = bazel_run_directories(std::env::var_os("BUILD_WORKING_DIRECTORY").as_deref(), &cwd, &binaries) else {
        return;
    };
    if std::env::var_os("RUNFILES_DIR").is_none() && std::env::var_os("RUNFILES_MANIFEST_FILE").is_none() {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { std::env::set_var("RUNFILES_DIR", &runfiles) };
    }
    if let Err(e) = std::env::set_current_dir(&wd) {
        eprintln!("warning: cannot enter BUILD_WORKING_DIRECTORY {}: {e}", wd.display());
    }
}

fn main() -> ExitCode {
    enter_bazel_run_working_directory();
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// The turn a unit quaternion carries, for a reader: the signed angle in
/// degrees, in (-180, 180], about a world axis when it is one
/// (`turned -90.000 deg about z`), else about the axis itself. `q` and `-q`
/// are the same rotation, so the angle is read with its axis rather than
/// from `w` alone, which loses the direction and folds 270 onto 90.
fn describe_turn(x: f64, y: f64, z: f64, w: f64) -> String {
    let n = (x * x + y * y + z * z).sqrt();
    let mut deg = 2.0 * n.atan2(w).to_degrees();
    let (mut ax, mut ay, mut az) = if n > 0.0 { (x / n, y / n, z / n) } else { (0.0, 0.0, 1.0) };
    if deg > 180.0 {
        deg = 360.0 - deg;
        (ax, ay, az) = (-ax, -ay, -az);
    }
    if deg.abs() < 1e-6 {
        return "no turn".to_owned();
    }
    for (name, c) in [("x", ax), ("y", ay), ("z", az)] {
        if c.abs() > 1.0 - 1e-9 {
            return format!("turned {:.3} deg about {name}", deg * c.signum());
        }
    }
    format!("turned {deg:.3} deg about ({ax:.4}, {ay:.4}, {az:.4})")
}

#[cfg(test)]
mod tests {
    #[test]
    fn describe_turn_keeps_the_direction() {
        // A turn of `deg` about +z, as a quaternion with w >= 0.
        let about_z = |deg: f64| {
            let h = deg.to_radians() / 2.0;
            let (mut z, mut w) = (h.sin(), h.cos());
            if w < 0.0 {
                (z, w) = (-z, -w);
            }
            describe_turn(0.0, 0.0, z, w)
        };
        assert_eq!(about_z(0.0), "no turn");
        assert_eq!(about_z(45.0), "turned 45.000 deg about z");
        assert_eq!(about_z(225.0), "turned -135.000 deg about z");
        assert_eq!(about_z(270.0), "turned -90.000 deg about z");
        assert_eq!(about_z(315.0), "turned -45.000 deg about z");
        assert_eq!(about_z(-30.0), "turned -30.000 deg about z");
        // The same rotation written as -q reads the same.
        let h = 135f64.to_radians();
        assert_eq!(describe_turn(0.0, 0.0, -h.sin(), -h.cos()), "turned -90.000 deg about z");
        // An axis that is not a world axis is printed.
        let s = 0.5f64.sqrt() * (30f64.to_radians() / 2.0).sin();
        assert_eq!(describe_turn(s, s, 0.0, (30f64.to_radians() / 2.0).cos()), "turned 30.000 deg about (0.7071, 0.7071, 0.0000)");
    }

    use super::*;
    use std::path::{Path, PathBuf};

    /// `animate` shares `render`'s view flags, but its frames are drawn by
    /// the engine's CPU renderers with no analysis or debug view: those
    /// flags (and `--backend gpu`) were accepted and silently ignored, so
    /// the file looked like the request had been honoured. They are refused.
    #[test]
    fn animate_refuses_the_view_flags_it_cannot_draw() {
        let refusal = |extra: &[&str]| {
            let mut args = vec!["cadpole", "animate", "x.star", "-o", "a.apng"];
            args.extend_from_slice(extra);
            match Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}")).cmd {
                Cmd::Animate { view, .. } => animate_refuses(&view),
                _ => unreachable!(),
            }
        };
        assert_eq!(refusal(&[]), None);
        assert_eq!(refusal(&["--backend", "path", "--xray"]), None);
        for (extra, want) in [
            (&["--analysis", "overhang"][..], "--analysis"),
            (&["--debug", "faces"][..], "--debug"),
            (&["--debug", "nosuchview"][..], "--debug"),
            (&["--backend", "gpu"][..], "--backend gpu"),
        ] {
            let why = refusal(extra).unwrap_or_else(|| panic!("{extra:?} is refused"));
            assert!(why.contains(want), "{extra:?}: {why}");
        }
    }

    /// `--max-heap-mb` is a global flag, before or after the
    /// subcommand, in MB; nothing below 1 MB, never "no cap"; and absent it is
    /// the 512 MB every other host keeps.
    #[test]
    fn the_heap_cap_is_a_global_flag_in_megabytes() {
        for args in [
            vec!["cadpole", "--max-heap-mb", "2048", "eval", "x.star"],
            vec!["cadpole", "eval", "x.star", "--max-heap-mb", "2048"],
            vec!["cadpole", "check", "x.star", "--max-heap-mb", "2048", "--layer", "kit"],
        ] {
            let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert_eq!(cli.max_heap_mb, Some(2048), "{args:?}");
        }
        assert_eq!(heap_bytes_of(2048), 2048 * 1024 * 1024);
        assert_eq!(heap_bytes_of(0), 1024 * 1024, "0 is 1 MB, not unlimited");
        assert_eq!(cadpole_engine::Limits::DEFAULT_HEAP_BYTES, 512 * 1024 * 1024);
    }

    /// `--eval-budget-secs` is a global flag in seconds, before or after the
    /// subcommand, on every evaluating command; `CADPOLE_EVAL_BUDGET_SECS`
    /// sets it too; 0 is no deadline; absent it is the 120 s batch budget.
    #[test]
    fn the_eval_budget_is_a_global_flag_in_seconds() {
        for args in [
            vec!["cadpole", "--eval-budget-secs", "900", "eval", "x.star"],
            vec!["cadpole", "export", "x.star", "-o", "x.3mf", "--eval-budget-secs", "900"],
            vec!["cadpole", "render", "x.star", "--eval-budget-secs", "900", "-o", "x.png"],
            vec!["cadpole", "check", "x.star", "--eval-budget-secs", "900", "--layer", "kit"],
            vec!["cadpole", "plate", "x.star", "--eval-budget-secs", "900", "-o", "x.3mf"],
            vec!["cadpole", "test", "x.star", "--eval-budget-secs", "900"],
            vec!["cadpole", "sweep", "x.star", "--param", "a=1,2", "--eval-budget-secs", "900", "-o", "s.png"],
        ] {
            let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert_eq!(cli.eval_budget_secs, Some(900), "{args:?}");
        }
        assert_eq!(eval_deadline_ms_of(Some(900)), 900_000);
        assert_eq!(eval_deadline_ms_of(Some(0)), 0, "0 is no deadline");
        assert_eq!(eval_deadline_ms_of(None), 120_000, "unset is the batch budget");
        assert_eq!(eval_deadline_ms_of(Some(u64::MAX)), u64::MAX, "saturates, does not wrap");
        // The env var, through clap's own lookup: a command that names it.
        let cmd = <Cli as clap::CommandFactory>::command();
        let arg = cmd.get_arguments().find(|a| a.get_id() == "eval_budget_secs").expect("the flag exists");
        assert_eq!(arg.get_env().and_then(|e| e.to_str()), Some("CADPOLE_EVAL_BUDGET_SECS"));
    }

    /// The per-body line `cadpole export` prints is a format scripts read, so
    /// it is pinned here: the id and name, the triangles, the tolerance and
    /// its level, the made-at tolerance when it differs, the chord error and its
    /// face; a mesh body has no chord error and STEP has none of it.
    #[test]
    fn the_export_report_line_is_stable() {
        let b = pb::ExportBodyReport {
            body_id: "sun#0".into(),
            name: "sun".into(),
            triangles: 12340,
            tolerance: 0.01,
            tolerance_source: "body".into(),
            tessellated_at: 0.01,
            chord_error: Some(0.00973),
            chord_error_face: "sun/side[0.3]".into(),
            chord_error_at: Some(pb::Vec3 { x: 1.0, y: 2.0, z: 3.0 }),
            exact: false,
        };
        assert_eq!(
            export_body_line(&b),
            "sun#0 \"sun\": 12340 triangles at 0.01 mm (body), chord error 0.0097 mm on sun/side[0.3]"
        );
        let moved = pb::ExportBodyReport { tessellated_at: 0.00973, tolerance_source: "call".into(), ..b.clone() };
        assert_eq!(
            export_body_line(&moved),
            "sun#0 \"sun\": 12340 triangles at 0.01 mm (call; made at 0.0097 mm), chord error 0.0097 mm on sun/side[0.3]"
        );
        let mesh = pb::ExportBodyReport {
            chord_error: None,
            chord_error_face: String::new(),
            tolerance: 0.05,
            tolerance_source: "document".into(),
            tessellated_at: 0.05,
            ..b.clone()
        };
        assert_eq!(
            export_body_line(&mesh),
            "sun#0 \"sun\": 12340 triangles at 0.05 mm (document), chord error not measured"
        );
        let step = pb::ExportBodyReport { exact: true, triangles: 0, ..b };
        assert_eq!(export_body_line(&step), "sun#0 \"sun\": exact surfaces (STEP takes no tolerance)");
    }

    /// `--shadow-study`: the default grid is the three seasonal
    /// dates of `--sun`'s year at 09:00, 12:00 and 15:00, three across;
    /// `--dates`/`--times` change the grid; `--at` is an explicit list in
    /// its own order, one row up to four, then a near-square grid; the
    /// site and the zone come from `--sun`, whose own instant is unused.
    #[test]
    fn shadow_study_instants_are_a_grid_or_a_list() {
        let spec = "38,-121,2026-06-21,12:00,-8";
        let (site, nine, cols) = shadow_study_instants(spec, None, None, &[]).unwrap();
        assert_eq!((site.latitude, site.longitude, site.tz_hours), (38.0, -121.0, -8.0));
        assert_eq!(cols, 3);
        let dates: Vec<String> = nine.iter().map(|(d, _)| d.to_string()).collect();
        assert_eq!(dates[0], "2026-06-21");
        assert_eq!(dates[3], "2026-03-20");
        assert_eq!(dates[6], "2026-12-21");
        assert_eq!(nine.iter().map(|(_, h)| *h).collect::<Vec<_>>()[..3], [9.0, 12.0, 15.0]);
        let (_, six, cols) = shadow_study_instants(spec, Some("2026-12-21,2026-03-20"), Some("10:30,14:00"), &[]).unwrap();
        assert_eq!(cols, 2);
        assert_eq!(six.len(), 4);
        assert_eq!((six[3].0.to_string(), six[3].1), ("2026-03-20".to_owned(), 14.0));
        let at = ["2026-12-21,12:00".to_owned(), "2026-03-20,09:00".to_owned(), "2026-03-20,15:00".to_owned()];
        let (_, three, cols) = shadow_study_instants(spec, None, None, &at).unwrap();
        assert_eq!(cols, 3);
        assert_eq!(three.iter().map(|(d, h)| format!("{d} {h}")).collect::<Vec<_>>(), ["2026-12-21 12", "2026-03-20 9", "2026-03-20 15"]);
        let seven: Vec<String> = (1..=7).map(|k| format!("2026-03-{k:02},12:00")).collect();
        assert_eq!(shadow_study_instants(spec, None, None, &seven).unwrap().2, 3);
        for (bad, needle) in [
            (("az=200,el=45", None, None, vec![]), "needs --sun lat,lon"),
            ((spec, None, None, vec!["2026-12-21".to_owned()]), "expected YYYY-MM-DD,HH:MM"),
            ((spec, None, None, vec!["2026-13-21,12:00".to_owned()]), "month 13"),
            ((spec, None, Some("25:00"), vec![]), "--times"),
            ((spec, Some("2026-02-30"), None, vec![]), "--dates"),
        ] {
            let err = shadow_study_instants(bad.0, bad.1, bad.2, &bad.3).unwrap_err().to_string();
            assert!(err.contains(needle), "{err}");
        }
    }

    #[test]
    fn two_script_exports_that_collide_under_out_dir_are_refused() {
        let dir = Path::new("/out");
        // Distinct file names are fine, directory part and all.
        let ok = crate::export_targets(&["a.stl", "sub/b.stl", "../../c.stl"], dir, None).unwrap();
        assert_eq!(
            ok,
            vec![PathBuf::from("/out/a.stl"), PathBuf::from("/out/b.stl"), PathBuf::from("/out/c.stl")],
            "the directory part is dropped, which is what keeps a script inside --out-dir",
        );

        // Same file name, different directory: an error, not x.stl written
        // twice with only the second body left.
        let err = crate::export_targets(&["x.stl", "sub/x.stl"], dir, None).unwrap_err().to_string();
        assert!(err.contains("/out/x.stl"), "{err}");
        assert!(err.contains("\"x.stl\"") && err.contains("\"sub/x.stl\""), "names both: {err}");

        // And a script export that would land on --out.
        let err = crate::export_targets(&["x.stl"], dir, Some(Path::new("/out/x.stl")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--out"), "{err}");
        // A different --out is not a collision.
        crate::export_targets(&["x.stl"], dir, Some(Path::new("/out/whole.stl"))).unwrap();
    }

    /// `--background #rrggbb` is typed in sRGB and comes out of the renderer
    /// as `to_srgb8(linear)`, so the request has to carry the decoded value
    /// for the pixel written to be the one asked for. Anything that is not
    /// six hex digits is refused rather than turned into grey.
    #[test]
    fn background_flag_decodes_srgb_and_refuses_junk() {
        let c = background_color("#ffffff").unwrap();
        assert_eq!((c.r, c.g, c.b), (1.0, 1.0, 1.0));
        let c = background_color("000000").unwrap();
        assert_eq!((c.r, c.g, c.b), (0.0, 0.0, 0.0));
        // Mid-grey is much darker in linear light than its byte suggests.
        let c = background_color("#808080").unwrap();
        assert!((c.r - 0.2158605).abs() < 1e-6, "{}", c.r);
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
        // And the decode round-trips through the encoder the exporters use,
        // which is the same curve the renderer writes pixels with.
        for h in ["#000000", "#010101", "#111111", "#404040", "#808080", "#C8C8C8", "#FEFEFE", "#FFFFFF"] {
            let c = background_color(h).unwrap();
            assert_eq!(cadpole_engine::cadpole_core::paint::hex([c.r, c.g, c.b]), h);
        }
        for bad in ["", "#fff", "#gggggg", "#12345678", "red"] {
            assert!(background_color(bad).is_err(), "{bad:?} must be refused");
        }
    }

    /// Both flags reach both requests: `cadpole render` and `cadpole sweep` share
    /// `ViewArgs`, so a flag added to one that the other silently dropped
    /// would be a trap.
    #[test]
    fn background_and_environment_reach_the_render_request() {
        // `ViewArgs` is only ever flattened into a subcommand, so parse it
        // through one here too.
        #[derive(Parser)]
        struct Only {
            #[command(flatten)]
            view: ViewArgs,
        }
        let eval = EvalArgs { input: "x.star".into(), params: vec![], tolerance: None, boolean_tolerance: None };
        let mut view = Only::parse_from(["cadpole"]).view;
        let req = render_request(&eval, &view, (64, 48)).unwrap();
        assert!(req.background.is_none(), "unset means the renderer's default");
        assert!(!req.show_environment);

        view.background = Some("#ffffff".into());
        view.environment = true;
        let req = render_request(&eval, &view, (64, 48)).unwrap();
        assert_eq!(req.background, Some(pb::Color { r: 1.0, g: 1.0, b: 1.0 }));
        assert!(req.show_environment);

        view.background = Some("#12".into());
        assert!(render_request(&eval, &view, (64, 48)).is_err(), "a bad colour fails the command, not the image");

        // `transparent` is a background of its own, not a colour.
        let view = Only::parse_from(["cadpole", "--background", "transparent", "--shadow-catcher", "--supersample", "2"]).view;
        let req = render_request(&eval, &view, (64, 48)).unwrap();
        assert!(req.transparent && req.shadow_catcher && req.background.is_none());
        assert_eq!(req.supersample, 2);
        let view = Only::parse_from(["cadpole", "--supersample", "5"]).view;
        let e = render_request(&eval, &view, (64, 48)).unwrap_err().to_string();
        assert!(e.contains("--supersample must be 1, 2, 3 or 4"), "{e}");
    }

    /// `--variant NAME=WxH` names a size, and the name goes into the
    /// output's path: a directory of its own for frames, a suffix for a file.
    #[test]
    fn a_variant_names_a_size_and_its_own_output() {
        assert_eq!(parse_variant("portrait=1080x1920").unwrap(), (Some("portrait".to_owned()), (1080, 1920)));
        for bad in ["portrait", "=10x10", "a/b=10x10", "wide=10"] {
            assert!(parse_variant(bad).is_err(), "{bad:?} must be refused");
        }
        let p = std::path::Path::new;
        assert_eq!(variant_path(p("out/frames"), Some("wide"), true), p("out/frames/wide"));
        assert_eq!(variant_path(p("out/toy.apng"), Some("wide"), false), p("out/toy_wide.apng"));
        assert_eq!(variant_path(p("out/cam.json"), Some("tall"), false), p("out/cam_tall.json"));
        assert_eq!(variant_path(p("out/toy.apng"), None, false), p("out/toy.apng"));
    }

    /// The camera sidecar is JSON a page can use without this crate: one
    /// entry per frame, the matrices as sixteen numbers in the order the
    /// file's own `matrices` note describes.
    #[test]
    fn the_camera_sidecar_is_one_json_entry_per_frame() {
        let v = |x, y, z| Some(pb::Vec3 { x, y, z });
        let frame = |index: u32| pb::CameraInfo {
            index,
            t: index as f64 * 0.5,
            width: 200,
            height: 100,
            eye: v(0.0, -10.0, 0.0),
            target: v(0.0, 0.0, 0.0),
            up: v(0.0, 0.0, 1.0),
            fov_degrees: 90.0,
            // Looking down +y from (0, -10, 0): right is +x, up is +z.
            view: vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, -10.0, 0.0, 0.0, 0.0, 1.0],
            projection: vec![0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, -0.2, 0.0, 0.0, -1.0, 0.0],
            ground_origin: v(0.0, 0.0, -1.0),
            ground_normal: v(0.0, 0.0, 1.0),
            ground_u: v(1.0, 0.0, 0.0),
            ground_v: v(0.0, 1.0, 0.0),
            bounds_min: v(-1.0, -1.0, -1.0),
            bounds_max: v(1.0, 1.0, 1.0),
            bounds_px: vec![90.0, 40.0, 110.0, 60.0],
            ..Default::default()
        };
        let doc: serde_json::Value = serde_json::from_str(&camera_json(&[frame(0), frame(1)])).unwrap();
        assert_eq!((doc["width"].as_u64(), doc["height"].as_u64(), doc["units"].as_str()), (Some(200), Some(100), Some("mm")));
        let frames = doc["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[1]["index"].as_u64(), frames[1]["t"].as_f64()), (Some(1), Some(0.5)));
        let f = &frames[0];
        assert_eq!(f["ground"]["origin"], serde_json::json!([0.0, 0.0, -1.0]));
        assert_eq!(f["bounds"]["px"], serde_json::json!([90.0, 40.0, 110.0, 60.0]));
        // A reader following the note: the point (5, 0, 5) is ten units
        // ahead, so through this 90 degree lens it is half way from the
        // centre to the top edge, and the same distance to the right.
        let m = |k: &str| -> Vec<f64> { f[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
        let mul = |m: &[f64], v: [f64; 4]| -> [f64; 4] { std::array::from_fn(|r| (0..4).map(|c| m[r * 4 + c] * v[c]).sum()) };
        let clip = mul(&m("projection"), mul(&m("view"), [5.0, 0.0, 5.0, 1.0]));
        let (x, y) = ((clip[0] / clip[3] + 1.0) / 2.0 * 200.0, (1.0 - clip[1] / clip[3]) / 2.0 * 100.0);
        assert!((x - 125.0).abs() < 1e-9 && (y - 25.0).abs() < 1e-9, "({x}, {y})");
    }

    /// The `cadpole` binary this test was built beside (`data = [":cadpole"]`),
    /// for the behaviour only a real process shows: exit codes, a closed pipe.
    pub(crate) fn cadpole_binary() -> Option<PathBuf> {
        let r = runfiles::Runfiles::create().ok()?;
        let p = runfiles::rlocation!(r, "cadpole/src/rust/cadpole-cli/cadpole")?;
        p.exists().then_some(p)
    }

    /// A session's event log as `cadpole listen` sees it: the server's
    /// contract for `WaitEvents` (ops.proto: `after` 0 with a client id
    /// resumes from the stored cursor, `kinds` decides when the poll ends,
    /// the cursor covers everything examined), `Ack` and `GetSession`.
    ///
    /// A poll that finds nothing takes the next step of `script` instead of
    /// waiting: more events, a failed call, or a restart. When the script
    /// runs out the reader is told its output was closed, which is how a
    /// test ends a `--follow`.
    struct FakeSession(std::sync::Mutex<FakeState>);

    #[derive(Default)]
    struct FakeState {
        id: String,
        log: Vec<pb::Event>,
        cursors: HashMap<String, u64>,
        listening: bool,
        script: std::collections::VecDeque<Step>,
        closed: bool,
        /// The `after` and `kinds` of every poll, as received.
        polls: Vec<(u64, Vec<String>)>,
    }

    enum Step {
        Events(Vec<(&'static str, serde_json::Value)>),
        Fail,
        Restart(&'static str),
    }

    impl FakeSession {
        fn new(listening: bool, script: Vec<Step>) -> FakeSession {
            FakeSession(std::sync::Mutex::new(FakeState { id: "run-1".into(), listening, script: script.into(), ..Default::default() }))
        }
        fn push(st: &mut FakeState, kind: &str, data: serde_json::Value) {
            let mut data = data;
            if kind == "assistant_listening" {
                // As the server writes it: the new value and the one before.
                data["was"] = serde_json::json!(st.listening);
                st.listening = data["listening"].as_bool().unwrap_or(false);
            }
            let seq = st.log.last().map(|e| e.seq).unwrap_or(0) + 1;
            st.log.push(pb::Event { seq, kind: kind.to_owned(), data: Some(cadpole_ops::json_to_struct(&data)), ..Default::default() });
        }
        /// Events already in the log before anyone listens.
        fn with_log(self, events: Vec<(&'static str, serde_json::Value)>) -> FakeSession {
            {
                let mut st = self.0.lock().unwrap();
                for (kind, data) in events {
                    FakeSession::push(&mut st, kind, data);
                }
            }
            self
        }
        fn closed(&self) -> bool {
            self.0.lock().unwrap().closed
        }
        fn cursor(&self, client: &str) -> u64 {
            self.0.lock().unwrap().cursors.get(client).copied().unwrap_or(0)
        }
    }

    impl Host for FakeSession {
        fn get_session(&self, _req: &pb::GetSessionRequest) -> anyhow::Result<pb::Session> {
            let st = self.0.lock().unwrap();
            Ok(pb::Session {
                id: st.id.clone(),
                last_event_seq: st.log.last().map(|e| e.seq).unwrap_or(0),
                assistant_listening: st.listening,
                ..Default::default()
            })
        }
        fn ack(&self, req: &pb::AckRequest) -> anyhow::Result<pb::AckResponse> {
            let mut st = self.0.lock().unwrap();
            let last = st.log.last().map(|e| e.seq).unwrap_or(0);
            let slot = st.cursors.entry(req.client.clone()).or_insert(0);
            *slot = (*slot).max(req.cursor.min(last));
            Ok(pb::AckResponse { client: req.client.clone(), cursor: *slot, last_event_seq: last, epoch_reset: req.cursor > last })
        }
        fn wait_events(&self, req: &pb::WaitEventsRequest) -> anyhow::Result<pb::WaitEventsResponse> {
            let mut st = self.0.lock().unwrap();
            st.polls.push((req.after, req.kinds.clone()));
            loop {
                let after = if req.after == 0 && !req.client.is_empty() { st.cursors.get(&req.client).copied().unwrap_or(0) } else { req.after };
                let backlog: Vec<pb::Event> = st.log.iter().filter(|e| e.seq > after).cloned().collect();
                let events: Vec<pb::Event> = backlog.iter().filter(|e| req.kinds.is_empty() || req.kinds.contains(&e.kind)).cloned().collect();
                let cursor = backlog.last().map(|e| e.seq).unwrap_or(after);
                if !events.is_empty() || req.timeout_ms == 1 {
                    return Ok(pb::WaitEventsResponse { events, cursor, ..Default::default() });
                }
                match st.script.pop_front() {
                    // Nothing more will happen: the poll times out empty.
                    None => {
                        st.closed = true;
                        return Ok(pb::WaitEventsResponse { cursor, ..Default::default() });
                    }
                    Some(Step::Events(events)) => {
                        for (kind, data) in events {
                            FakeSession::push(&mut st, kind, data);
                        }
                    }
                    Some(Step::Fail) => anyhow::bail!("connection refused"),
                    Some(Step::Restart(id)) => {
                        st.id = id.to_owned();
                        st.log.clear();
                        st.cursors.clear();
                        anyhow::bail!("connection refused");
                    }
                }
            }
        }
    }

    /// Run `listen` against a fake to the end of its script; the printed
    /// lines (parsed) and the notes.
    fn run_listen(session: &std::sync::Arc<FakeSession>, mode: ListenMode, filter: ListenFilter, configure: impl FnOnce(&mut ListenOpts)) -> (Vec<serde_json::Value>, Vec<String>) {
        struct Shared(std::sync::Arc<FakeSession>);
        impl Host for Shared {
            fn get_session(&self, req: &pb::GetSessionRequest) -> anyhow::Result<pb::Session> {
                self.0.get_session(req)
            }
            fn ack(&self, req: &pb::AckRequest) -> anyhow::Result<pb::AckResponse> {
                self.0.ack(req)
            }
            fn wait_events(&self, req: &pb::WaitEventsRequest) -> anyhow::Result<pb::WaitEventsResponse> {
                self.0.wait_events(req)
            }
        }
        let mut opts = ListenOpts { mode, filter, after: None, always_listen: false, client: "listen".into(), retry: std::time::Duration::ZERO };
        configure(&mut opts);
        let (mut lines, mut notes) = (Vec::new(), Vec::new());
        let connect = || -> Box<dyn Host> { Box::new(Shared(session.clone())) };
        listen(&connect, &opts, &mut |l| lines.push(serde_json::from_str(&l).expect("one JSON object per line")), &mut |n| notes.push(n), &|| session.closed())
            .expect("listen");
        (lines, notes)
    }

    fn user(text: &str) -> (&'static str, serde_json::Value) {
        ("message", serde_json::json!({"from": "user", "text": text}))
    }

    /// `cadpole listen` without a mode flag returns after the current
    /// backlog, at once, also when there is none, and prints every kind.
    #[test]
    fn listen_without_a_mode_prints_the_backlog_and_does_not_wait() {
        let empty = std::sync::Arc::new(FakeSession::new(false, vec![]));
        let (lines, notes) = run_listen(&empty, ListenMode::Backlog, ListenFilter::All, |_| {});
        assert!(lines.is_empty() && notes.is_empty(), "{lines:?} {notes:?}");
        assert!(!empty.closed(), "an empty backlog must not be waited on");

        let log = vec![("params_changed", serde_json::json!({})), user("a"), ("selection_changed", serde_json::json!({}))];
        let s = std::sync::Arc::new(FakeSession::new(false, vec![]).with_log(log));
        let (lines, _) = run_listen(&s, ListenMode::Backlog, ListenFilter::All, |_| {});
        assert_eq!(lines.len(), 3, "every kind, Listen toggle or not: {lines:?}");
        let (lines, _) = run_listen(&s, ListenMode::Backlog, ListenFilter::All, |o| o.after = Some(1));
        assert_eq!(lines.len(), 2, "the backlog after seq 1: {lines:?}");
        assert_eq!(s.cursor("listen"), 0, "reading the backlog moves nobody's cursor");
    }

    /// The property a watcher of `cadpole listen --follow` pays for: with
    /// nothing happening, nothing is printed, on either stream.
    #[test]
    fn a_follow_prints_nothing_while_nothing_happens() {
        let s = std::sync::Arc::new(FakeSession::new(true, vec![]).with_log(vec![user("before the watch started")]));
        let (lines, notes) = run_listen(&s, ListenMode::Follow, ListenFilter::Wake, |_| {});
        assert!(lines.is_empty(), "no heartbeat, no banner, no backlog: {lines:?}");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(s.cursor("listen"), 1, "a new client adopts the end of the log and records it");
    }

    /// One line per event worth a model turn, and none for the rest: a bare
    /// selection, a parameter drag, a script edit and the assistant's own
    /// reply are context, not a reason to wake anyone.
    #[test]
    fn a_follow_prints_one_line_per_actionable_event_and_nothing_else() {
        let script = vec![Step::Events(vec![
            ("selection_changed", serde_json::json!({"selected": [{"feature_id": "plate"}]})),
            ("params_changed", serde_json::json!({"overrides": {"t": 5}})),
            ("script_changed", serde_json::json!({"path": "main.star", "revision": 2})),
            ("message", serde_json::json!({"from": "assistant", "text": "done"})),
            user("make the plate thicker"),
            ("selection_changed", serde_json::json!({"selected": [{"feature_id": "plate"}], "note": "too thin"})),
            ("selection_changed", serde_json::json!({"selected": [], "note": ""})),
        ])];
        let s = std::sync::Arc::new(FakeSession::new(true, script));
        let (lines, notes) = run_listen(&s, ListenMode::Follow, ListenFilter::Wake, |_| {});
        let got: Vec<(&str, &str)> = lines
            .iter()
            .map(|l| (l["kind"].as_str().unwrap(), l["data"]["text"].as_str().or(l["data"]["note"].as_str()).unwrap()))
            .collect();
        assert_eq!(got, [("message", "make the plate thicker"), ("selection_changed", "too thin")]);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(s.cursor("listen"), 7, "what was not printed was still read");
        // The server is asked for these kinds only, so a parameter drag does
        // not even end the long poll.
        let st = s.0.lock().unwrap();
        assert!(st.polls.iter().all(|(_, kinds)| kinds == &["message", "selection_changed", "assistant_listening"]), "{:?}", st.polls);
    }

    /// `--kinds` and `--all` widen what is printed.
    #[test]
    fn the_listen_filter_can_be_widened() {
        let events = || vec![Step::Events(vec![("params_changed", serde_json::json!({})), ("selection_changed", serde_json::json!({})), user("x")])];
        let s = std::sync::Arc::new(FakeSession::new(true, events()));
        let (lines, _) = run_listen(&s, ListenMode::Follow, ListenFilter::Kinds(vec!["params_changed".into(), "selection_changed".into()]), |_| {});
        assert_eq!(lines.iter().map(|l| l["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["params_changed", "selection_changed"]);
        let s = std::sync::Arc::new(FakeSession::new(true, events()));
        let (lines, _) = run_listen(&s, ListenMode::Follow, ListenFilter::All, |_| {});
        assert_eq!(lines.len(), 3, "{lines:?}");
    }

    /// The UI's Listen toggle is the user's switch: off, nothing is printed
    /// (and the one note about it goes to stderr); turned on, what follows
    /// is; `--always-listen` ignores it.
    #[test]
    fn a_follow_respects_the_listen_toggle() {
        let script = || {
            vec![
                Step::Events(vec![user("while off")]),
                Step::Events(vec![("assistant_listening", serde_json::json!({"listening": true})), user("while on")]),
                Step::Events(vec![("assistant_listening", serde_json::json!({"listening": false})), user("off again")]),
            ]
        };
        let s = std::sync::Arc::new(FakeSession::new(false, script()));
        let (lines, notes) = run_listen(&s, ListenMode::Follow, ListenFilter::Wake, |_| {});
        assert_eq!(lines.iter().map(|l| l["data"]["text"].as_str().unwrap()).collect::<Vec<_>>(), ["while on"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("Listen toggle is off"), "{notes:?}");

        let s = std::sync::Arc::new(FakeSession::new(false, script()));
        let (lines, notes) = run_listen(&s, ListenMode::Follow, ListenFilter::Wake, |o| o.always_listen = true);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// `--next` is the wait a harness without a line watcher uses: silent
    /// until something actionable, then that, then exit. The position is the
    /// server's, so the next run starts after it and nothing posted between
    /// two runs is lost.
    #[test]
    fn next_returns_the_next_actionable_event_and_resumes_after_it() {
        let script = vec![
            Step::Events(vec![("params_changed", serde_json::json!({}))]),
            Step::Events(vec![user("first"), user("second, same moment")]),
            Step::Events(vec![user("never reached by the first run")]),
        ];
        let s = std::sync::Arc::new(FakeSession::new(true, script));
        let (lines, _) = run_listen(&s, ListenMode::Next, ListenFilter::Wake, |_| {});
        assert_eq!(lines.iter().map(|l| l["data"]["text"].as_str().unwrap()).collect::<Vec<_>>(), ["first", "second, same moment"]);
        assert!(!s.closed(), "it returned because it printed, not because the script ran out");
        assert_eq!(s.cursor("listen"), 3);

        // Posted while nobody was waiting.
        FakeSession::push(&mut s.0.lock().unwrap(), "message", serde_json::json!({"from": "user", "text": "in between"}));
        let (lines, _) = run_listen(&s, ListenMode::Next, ListenFilter::Wake, |_| {});
        assert_eq!(lines.iter().map(|l| l["data"]["text"].as_str().unwrap()).collect::<Vec<_>>(), ["in between"]);
        // Another reader has its own position.
        let (lines, _) = run_listen(&s, ListenMode::Next, ListenFilter::Wake, |o| o.client = "second-watcher".into());
        assert_eq!(lines.iter().map(|l| l["data"]["text"].as_str().unwrap()).collect::<Vec<_>>(), ["never reached by the first run"]);
    }

    /// An event is judged by the toggle it was sent under, not by the toggle
    /// as it is when a run starts: what was typed while Listen was off stays
    /// unprinted after Listen is turned on, and what was typed while it was
    /// on is still printed after it is turned off (spec/wakeups.qnt,
    /// `everyHandOverWasSentWhileListening` and `nothingActionableIsSkipped`).
    #[test]
    fn a_run_judges_a_backlog_by_the_toggle_each_event_was_sent_under() {
        let between_runs = |listening: bool, text: &str| {
            let s = std::sync::Arc::new(FakeSession::new(listening, vec![]).with_log(vec![("params_changed", serde_json::json!({}))]));
            // A first run, so the client has a place in the log.
            let (lines, _) = run_listen(&s, ListenMode::Next, ListenFilter::Wake, |_| {});
            assert!(lines.is_empty(), "{lines:?}");
            {
                let mut st = s.0.lock().unwrap();
                FakeSession::push(&mut st, "message", serde_json::json!({"from": "user", "text": text}));
                FakeSession::push(&mut st, "assistant_listening", serde_json::json!({"listening": !listening}));
            }
            let (lines, _) = run_listen(&s, ListenMode::Next, ListenFilter::Wake, |_| {});
            lines.iter().map(|l| l["data"]["text"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        };
        assert!(between_runs(false, "typed while Listen was off").is_empty(), "turning Listen on afterwards does not hand it over");
        assert_eq!(between_runs(true, "typed while Listen was on"), ["typed while Listen was on"], "turning Listen off afterwards does not lose it");
    }

    /// A server that stops answering is waited for, quietly on stdout; one
    /// that comes back as a new run is read from the start of its new log.
    #[test]
    fn a_follow_survives_a_server_that_goes_away() {
        let script = vec![
            Step::Events(vec![user("one")]),
            Step::Fail,
            Step::Events(vec![user("two")]),
            Step::Restart("run-2"),
            Step::Events(vec![("assistant_listening", serde_json::json!({"listening": true})), user("three")]),
        ];
        let s = std::sync::Arc::new(FakeSession::new(true, script).with_log(vec![user("old"), user("older")]));
        let (lines, notes) = run_listen(&s, ListenMode::Follow, ListenFilter::Wake, |_| {});
        assert_eq!(lines.iter().map(|l| l["data"]["text"].as_str().unwrap()).collect::<Vec<_>>(), ["one", "two", "three"]);
        assert_eq!(lines[2]["seq"], serde_json::json!(2), "the new log's own numbering: {}", lines[2]);
        let notes = notes.join("\n");
        assert!(notes.contains("stopped answering") && notes.contains("answering again") && notes.contains("restarted"), "{notes}");
    }

    /// With no server at all there is nothing to wait for: an error, at once.
    #[test]
    fn listen_fails_at_once_when_there_is_no_session() {
        let connect = || -> Box<dyn Host> { Box::new(cadpole_ops::NoHost) };
        let opts = ListenOpts { mode: ListenMode::Follow, filter: ListenFilter::Wake, after: None, always_listen: false, client: "listen".into(), retry: std::time::Duration::ZERO };
        assert!(listen(&connect, &opts, &mut |_| {}, &mut |_| {}, &|| false).is_err());
    }

    /// `cadpole sweep` takes the render flags, but its request carries only
    /// some of them: a flag it cannot carry is refused, by name, rather than
    /// accepted and dropped (an invalid value included).
    #[test]
    fn sweep_refuses_the_view_flags_it_cannot_carry() {
        let sweep = |extra: &[&str]| {
            let mut args = vec!["cadpole", "sweep", "x.star", "--param", "w=1,2", "-o", "s.png"];
            args.extend_from_slice(extra);
            let Cmd::Sweep { eval, view, tile, cols, .. } = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}")).cmd else {
                unreachable!()
            };
            sweep_request(&eval, &view, tile.as_deref(), cols)
        };
        sweep(&[]).unwrap();
        sweep(&["--backend", "raster", "--view", "top", "--sun", "az=10,el=40", "--xray", "--tonemap", "neutral", "--fov", "20"]).unwrap();
        for (flags, name) in [
            (&["--section", "z=40"][..], "--section"),
            (&["--analysis", "thickness"], "--analysis"),
            (&["--analysis", "bogus"], "--analysis"),
            (&["--min-wall", "1"], "--min-wall"),
            (&["--max-overhang", "50"], "--max-overhang"),
            (&["--debug", "wire"], "--debug"),
            (&["--backend", "path"], "--backend"),
            (&["--backend", "bogus"], "--backend"),
            (&["--bounces", "4"], "--bounces"),
            (&["--compare-edges"], "--compare-edges"),
            (&["--bloom"], "--bloom"),
            (&["--dof", "4"], "--dof"),
            (&["--edge-break"], "--edge-break"),
            (&["--denoise"], "--denoise"),
            (&["--stats"], "--stats"),
        ] {
            let e = sweep(flags).expect_err(&format!("{flags:?} must be refused")).to_string();
            assert!(e.contains(name) && e.contains("sweep"), "{flags:?}: {e}");
        }
    }

    /// A scratch directory for one test, empty.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadpole-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// `BUILD_WORKING_DIRECTORY` counts only when the process is sitting in
    /// its own runfiles tree, which is how `bazel run` starts it; anything a
    /// `bazel run` tool launches elsewhere inherits the variable and keeps
    /// its own directory.
    #[test]
    fn bazel_run_directory_is_taken_only_from_inside_the_runfiles_tree() {
        let dir = scratch("bazel-run-decide");
        let bin = dir.join("cadpole");
        let tree = dir.join("cadpole.runfiles");
        std::fs::create_dir_all(tree.join("_main")).unwrap();
        let wd = std::ffi::OsStr::new("/somewhere/typed");
        let tree = std::fs::canonicalize(&tree).unwrap();
        assert_eq!(
            bazel_run_directories(Some(wd), &dir.join("cadpole.runfiles/_main"), std::slice::from_ref(&bin)),
            Some((PathBuf::from("/somewhere/typed"), tree.clone()))
        );
        assert_eq!(bazel_run_directories(Some(wd), &dir.join("cadpole.runfiles"), std::slice::from_ref(&bin)), Some((PathBuf::from("/somewhere/typed"), tree)));
        assert_eq!(bazel_run_directories(Some(wd), &dir, std::slice::from_ref(&bin)), None, "outside the tree");
        assert_eq!(bazel_run_directories(None, &dir.join("cadpole.runfiles/_main"), std::slice::from_ref(&bin)), None);
        assert_eq!(bazel_run_directories(Some(std::ffi::OsStr::new("")), &dir.join("cadpole.runfiles/_main"), &[bin]), None);
    }

    /// The real process, started the way `bazel run` starts it: relative
    /// inputs and outputs land in `BUILD_WORKING_DIRECTORY`, not in the
    /// runfiles tree; and the same variable from a parent is ignored when
    /// the process starts anywhere else.
    #[test]
    fn bazel_run_resolves_paths_against_the_typed_directory() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("bazel-run-paths");
        // `bazel run` names the binary by an absolute path beside its
        // `.runfiles` tree; a symlink stands in for that layout here.
        let link = dir.join("cadpole");
        std::os::unix::fs::symlink(std::fs::canonicalize(&bin).unwrap(), &link).unwrap();
        let runfiles = dir.join("cadpole.runfiles/_main");
        std::fs::create_dir_all(&runfiles).unwrap();
        let typed = dir.join("typed");
        std::fs::create_dir_all(typed.join("models")).unwrap();
        std::fs::write(typed.join("models/part.star"), "a = box(10, 10, 10)\n").unwrap();

        let out = std::process::Command::new(&link)
            .args(["export", "models/part.star", "-o", "part.stl"])
            .current_dir(&runfiles)
            .env("BUILD_WORKING_DIRECTORY", &typed)
            .env_remove("RUNFILES_DIR")
            .env_remove("RUNFILES_MANIFEST_FILE")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(typed.join("part.stl").exists(), "the output is written where the command was typed");
        assert!(!runfiles.join("part.stl").exists());

        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("own.star"), "a = box(5, 5, 5)\n").unwrap();
        let out = std::process::Command::new(&link).args(["eval", "own.star"]).current_dir(&elsewhere).env("BUILD_WORKING_DIRECTORY", &typed).output().unwrap();
        assert!(out.status.success(), "an inherited variable moves nothing: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A script that fails to evaluate gets no image: `render` exits 1 and
    /// writes nothing, as `export` and `animate` do, rather than "wrote
    /// x.png" over an empty or partial scene. The contact-sheet forms too.
    #[test]
    fn render_writes_no_image_when_the_script_fails() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("render-error");
        std::fs::write(dir.join("ok.star"), "a = box(10, 10, 10)\n").unwrap();
        std::fs::write(dir.join("bad_after.star"), "a = box(10, 10, 10)\nb = box(10, 10, -1)\n").unwrap();
        std::fs::write(dir.join("bad_syntax.star"), "a = box(10, 10, 10)\nb = (\n").unwrap();
        let render = |script: &str, extra: &[&str]| {
            let out = dir.join(format!("{script}.png"));
            let _ = std::fs::remove_file(&out);
            let mut args = vec!["render".to_owned(), script.to_owned(), "--size".into(), "64x48".into(), "-o".into(), out.display().to_string()];
            args.extend(extra.iter().map(|s| s.to_string()));
            let o = std::process::Command::new(&bin).args(&args).current_dir(&dir).output().unwrap();
            (o.status.code(), out.exists(), String::from_utf8_lossy(&o.stderr).into_owned())
        };
        let (code, wrote, err) = render("ok.star", &[]);
        assert_eq!((code, wrote), (Some(0), true), "{err}");
        for (script, extra) in [
            ("bad_after.star", &[][..]),
            ("bad_syntax.star", &[]),
            ("bad_after.star", &["--compare-edges"]),
            ("bad_after.star", &["--shadow-study", "--sun", "45.0,-100.0,2026-06-21,12:00,-6", "--at", "2026-06-21,12:00"]),
        ] {
            let (code, wrote, err) = render(script, extra);
            assert_eq!(code, Some(1), "{script} {extra:?}: {err}");
            assert!(!wrote, "{script} {extra:?}: an image was written: {err}");
            assert!(!err.contains("wrote"), "{script} {extra:?}: {err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Render flags whose value means nothing are refused, naming the flag,
    /// rather than quietly replaced by a default (a typo'd view gave iso, an
    /// unknown backend raster, 0x0 the 800x600 default) or passed through to
    /// a blank or black image (fov 180, exposure inf).
    #[test]
    fn nonsense_render_flags_are_refused() {
        #[derive(Parser)]
        struct Only {
            #[command(flatten)]
            view: ViewArgs,
        }
        let eval = EvalArgs { input: "x.star".into(), params: vec![], tolerance: None, boolean_tolerance: None };
        let req = |flags: &[&str]| {
            let view = Only::try_parse_from(std::iter::once("cadpole").chain(flags.iter().copied())).unwrap().view;
            render_request(&eval, &view, (64, 48))
        };
        for ok in [
            &[][..],
            &["--backend", "gpu"],
            &["--backend", "path"],
            &["--view", "top"],
            &["--view", "iso-bottom"],
            &["--view", "hero"],
            &["--view", "az=35,el=-25"],
            &["--fov", "20"],
            &["--exposure", "1.5"],
        ] {
            req(ok).unwrap_or_else(|e| panic!("{ok:?}: {e}"));
        }
        for (bad, flag) in [
            (&["--backend", "nope"][..], "--backend"),
            (&["--view", "nope"], "--view"),
            (&["--view", "az=nan,el=0"], "--view"),
            (&["--view", "az=inf"], "--view"),
            (&["--fov", "0"], "--fov"),
            (&["--fov", "nan"], "--fov"),
            (&["--fov", "180"], "--fov"),
            (&["--fov", "inf"], "--fov"),
            (&["--exposure", "inf"], "--exposure"),
            (&["--exposure", "nan"], "--exposure"),
            (&["--exposure", "0"], "--exposure"),
            (&["--exposure=-1"], "--exposure"),
        ] {
            let e = req(bad).expect_err(&format!("{bad:?} must be refused")).to_string();
            assert!(e.contains(flag), "{bad:?}: {e}");
        }
        for bad in ["0x0", "0x100", "100x0", "x", "10"] {
            assert!(parse_size(bad).is_err(), "--size {bad}");
        }
        assert_eq!(parse_size("640x480").unwrap(), (640, 480));

        // `cadpole check`: the plate shape is one of the shapes, the bed is
        // positive finite millimetres.
        assert_eq!(bed_shape_arg(Some("circular".into())).unwrap(), "circular");
        assert_eq!(bed_shape_arg(None).unwrap(), "");
        assert!(bed_shape_arg(Some("hex".into())).unwrap_err().to_string().contains("--bed-shape"));
        assert_eq!(bed_arg(Some("256x256x256")).unwrap(), vec![256.0; 3]);
        for bad in ["nanxnanxnan", "inf", "0,10,10", "-5"] {
            assert!(bed_arg(Some(bad)).is_err(), "--bed {bad}");
        }

        // The output is a PNG whatever the name says, so a name that says
        // otherwise is refused; no extension is fine.
        for ok in ["a.png", "A.PNG", "out"] {
            png_out(Path::new(ok)).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for bad in ["a.jpg", "a.jpeg", "a.webp"] {
            assert!(png_out(Path::new(bad)).is_err(), "{bad}");
        }

        // A `--layer` the document does not declare is an error listing the
        // ones it does (the rule `check` and `export` follow); `other` is
        // always there.
        let model = pb::Model { layers: vec![pb::Layer { name: "envelope".into(), ..Default::default() }], ..Default::default() };
        let states = |names: &[&str]| names.iter().map(|n| pb::LayerState { name: (*n).into(), ..Default::default() }).collect::<Vec<_>>();
        check_layer_names(&states(&["envelope", "other"]), Some(&model)).unwrap();
        let e = check_layer_names(&states(&["nope"]), Some(&model)).unwrap_err().to_string();
        assert!(e.contains("nope") && e.contains("envelope"), "{e}");
    }

    /// A `--layer` the document does not declare is refused on every path
    /// that takes one, not only the plain render: the contact sheets, a
    /// sweep and an animation used to ignore the typo and write the image.
    #[test]
    fn an_undeclared_layer_is_refused_on_every_render_path() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("layer-paths");
        std::fs::write(dir.join("ok.star"), "q = param(\"q\", 1)\na = box(10, 10, 10)\n").unwrap();
        for args in [
            &["render", "ok.star", "-o", "x.png", "--size", "64x48", "--layer", "nope=off", "--compare-edges"][..],
            &["sweep", "ok.star", "--param", "q=1,2", "-o", "s.png", "--layer", "nope=off"],
        ] {
            let o = std::process::Command::new(&bin).args(args).current_dir(&dir).output().unwrap();
            let err = String::from_utf8_lossy(&o.stderr);
            assert_ne!(o.status.code(), Some(0), "{args:?}: {err}");
            assert!(err.contains("nope") && err.contains("declares no layer"), "{args:?}: {err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// No command declares a flag twice. clap checks this only when a
    /// command is first parsed, and only when it is compiled with debug
    /// assertions, so a clash on a command no test parses reaches a user as
    /// a panic at start-up, or, without the assertions, as a flag whose
    /// second meaning is silently never reached. Checked here for every
    /// command, whatever clap was compiled with.
    #[test]
    fn no_command_declares_a_flag_twice() {
        fn walk(cmd: &clap::Command, path: &str, clashes: &mut Vec<String>) {
            let mut seen: std::collections::BTreeMap<String, String> = Default::default();
            for arg in cmd.get_arguments() {
                let id = arg.get_id().to_string();
                let longs = arg.get_long().into_iter().chain(arg.get_all_aliases().unwrap_or_default()).map(|l| format!("--{l}"));
                let shorts = arg.get_short().into_iter().chain(arg.get_all_short_aliases().unwrap_or_default()).map(|c| format!("-{c}"));
                for flag in longs.chain(shorts).collect::<Vec<_>>() {
                    if let Some(first) = seen.insert(flag.clone(), id.clone()) {
                        clashes.push(format!("{path}: {flag} is both `{first}` and `{id}`"));
                    }
                }
            }
            let mut names: std::collections::BTreeSet<String> = Default::default();
            for sub in cmd.get_subcommands() {
                for name in std::iter::once(sub.get_name()).chain(sub.get_all_aliases()) {
                    if !names.insert(name.to_owned()) {
                        clashes.push(format!("{path}: two subcommands answer to `{name}`"));
                    }
                }
                walk(sub, &format!("{path} {}", sub.get_name()), clashes);
            }
        }
        for mut cmd in [<Cli as clap::CommandFactory>::command(), op_cli::command()] {
            assert!(cmd.get_subcommands().count() > 5, "{}: the walk sees its subcommands", cmd.get_name());
            // As declared, and again once building has handed the global
            // flags down to every subcommand. Building also runs clap's own
            // checks on the whole tree, where clap has them.
            for built in [false, true] {
                if built {
                    cmd.build();
                }
                let mut clashes = Vec::new();
                walk(&cmd, cmd.get_name(), &mut clashes);
                assert!(clashes.is_empty(), "{}", clashes.join("\n"));
            }
            cmd.debug_assert();
        }
    }

    /// `cadpole export` end to end: `--layer NAME` selects the bodies a mesh
    /// holds, `--layer NAME=off` switches a layer of a `--view` drawing, and
    /// each spelling is refused where the other is meant.
    #[test]
    fn export_takes_layer_for_a_mesh_and_for_a_drawing() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("export-layer");
        let script = "kit = layer(\"kit\")\na = box(10, 10, 10, layer=kit)\nb = cylinder(4, 30, at=(30, 0, 0))\n";
        std::fs::write(dir.join("ok.star"), script).unwrap();
        let run = |args: &[&str]| {
            let o = std::process::Command::new(&bin).arg("export").arg("ok.star").args(args).current_dir(&dir).output().unwrap();
            (o.status.code(), String::from_utf8_lossy(&o.stderr).into_owned())
        };
        let size = |name: &str| std::fs::metadata(dir.join(name)).map(|m| m.len()).unwrap_or(0);

        let (code, err) = run(&["-o", "all.stl"]);
        assert_eq!(code, Some(0), "{err}");
        let (code, err) = run(&["--layer", "kit", "-o", "kit.stl"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(size("kit.stl") > 0 && size("kit.stl") < size("all.stl"), "one body of two: {} of {}", size("kit.stl"), size("all.stl"));

        let (code, err) = run(&["--view", "top", "--layer", "kit=off", "-o", "plan.svg"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(size("plan.svg") > 0);

        let (code, err) = run(&["--layer", "kit=off", "-o", "no.stl"]);
        assert_ne!(code, Some(0));
        assert!(err.contains("takes a layer's name"), "{err}");
        let (code, err) = run(&["--view", "top", "--layer", "kit", "-o", "no.svg"]);
        assert_ne!(code, Some(0));
        assert!(err.contains("NAME=on"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `eval --model-out` names the file it could not write.
    #[test]
    fn a_model_out_that_cannot_be_written_is_named() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("model-out");
        std::fs::write(dir.join("ok.star"), "a = box(10, 10, 10)\n").unwrap();
        let o = std::process::Command::new(&bin)
            .args(["eval", "ok.star", "--model-out", "missing/x.pb"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(2), "{err}");
        assert!(err.contains("missing/x.pb"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--section -x=22` is the spelling the help shows for keeping the +x
    /// side, so the parser has to take it as the flag's value and not as a
    /// short flag `-x`.
    #[test]
    fn a_negative_section_is_the_flags_value() {
        for verb in [&["render", "x.star", "-o", "a.png"][..], &["animate", "x.star", "-o", "a.gif"]] {
            let mut args = vec!["cadpole"];
            args.extend_from_slice(verb);
            args.extend_from_slice(&["--section", "-x=20"]);
            let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let view = match cli.cmd {
                Cmd::Render { view, .. } | Cmd::Animate { view, .. } => view,
                _ => unreachable!(),
            };
            assert_eq!(view.section.as_deref(), Some("-x=20"), "{args:?}");
        }
    }

    /// `cadpole doc | head -1`: the reader has what it wanted and closed the
    /// pipe. The writer stops quietly, as Unix tools do, instead of
    /// panicking with exit 101 (which fails a `set -o pipefail` script).
    #[test]
    fn a_closed_stdout_ends_the_command_quietly() {
        let Some(bin) = cadpole_binary() else { return };
        let dir = scratch("closed-pipe");
        std::fs::write(dir.join("ok.star"), "a = box(10, 10, 10)\n").unwrap();
        for args in [&["doc"][..], &["doc", "--json"], &["op", "Evaluate", "--doc", "ok.star"]] {
            let mut child = std::process::Command::new(&bin)
                .args(args)
                .current_dir(&dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stdout.take());
            let out = child.wait_with_output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(!err.contains("panicked"), "{args:?}: {err}");
            assert_eq!(out.status.code(), Some(0), "{args:?}: {err}");
        }
        // A closed pipe does not turn a failure into success.
        std::fs::write(dir.join("fails.star"), "a = box(10, 10, 10)\ndef t():\n    assert_true(False, msg=\"no\")\ntest(\"t\", t)\n").unwrap();
        for args in [&["test", "fails.star"][..], &["op", "RunTests", "--doc", "fails.star", "--compact"]] {
            let mut child = std::process::Command::new(&bin)
                .args(args)
                .current_dir(&dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stdout.take());
            let out = child.wait_with_output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(!err.contains("panicked"), "{args:?}: {err}");
            assert_eq!(out.status.code(), Some(1), "{args:?}: {err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn gpu_request(src: &str, f: impl FnOnce(&mut pb::RenderRequest)) -> pb::RenderRequest {
        let mut req = pb::RenderRequest {
            document: Some(cadpole_engine::document_from_scripts("t", vec![("main.star".into(), src.into())])),
            width: 64,
            height: 48,
            ..Default::default()
        };
        f(&mut req);
        req
    }

    /// `--backend gpu` prepares the scene the way the engine's `Render` does:
    /// a `--section` cuts it, the document's `render_defaults(camera=)` is
    /// the camera when the request names none, and a sun -- which the wgpu
    /// renderer cannot draw -- sends the render to the CPU rasterizer rather
    /// than dropping it.
    #[test]
    fn the_gpu_backend_honours_section_sun_and_document_defaults() {
        const BOX: &str = "b = box(40, 40, 20)\n";
        let top = |g: &GpuScene| g.scene.bbox.expect("geometry").1.z;
        let GpuPlan::Draw(plain) = gpu_scene(&gpu_request(BOX, |_| {})).unwrap() else { panic!("a plain render draws") };
        assert!((top(&plain) - 20.0).abs() < 1e-6, "{}", top(&plain));
        let GpuPlan::Draw(cut) = gpu_scene(&gpu_request(BOX, |r| r.section = "z=10".into())).unwrap() else {
            panic!("a section draws on the gpu")
        };
        assert!(top(&cut) <= 10.0 + 1e-6, "--section z=10 cuts the scene: top at {}", top(&cut));

        let sun = gpu_scene(&gpu_request(BOX, |r| r.sun = "az=120,el=30".into())).unwrap();
        assert!(matches!(sun, GpuPlan::Cpu(ref why) if why.contains("sun")), "--sun goes to the rasterizer");
        let doc_sun = gpu_scene(&gpu_request(&format!("{BOX}render_defaults(sun=sun(az=200, el=50))\n"), |_| {})).unwrap();
        assert!(matches!(doc_sun, GpuPlan::Cpu(ref why) if why.contains("sun")), "render_defaults(sun=) too");

        let src = format!("{BOX}render_defaults(camera=view(eye=(200, 10, 5), target=(0, 0, 10), fov=60))\n");
        let GpuPlan::Draw(g) = gpu_scene(&gpu_request(&src, |_| {})).unwrap() else { panic!("draws") };
        let e = g.camera.eye;
        assert!((e.x - 200.0).abs() < 1e-9 && (e.y - 10.0).abs() < 1e-9 && (e.z - 5.0).abs() < 1e-9, "document camera: {e:?}");
        assert!((g.camera.fov_degrees - 60.0).abs() < 1e-9);
        let GpuPlan::Draw(g) = gpu_scene(&gpu_request(&src, |r| r.camera = Some(pb::Camera { preset: "top".into(), ..Default::default() }))).unwrap() else {
            panic!("draws")
        };
        assert!((g.camera.eye.x - 200.0).abs() > 1.0, "the request's own camera wins over the document's");
    }

    /// The three front-ends are generated
    /// from one registry, so their name sets are equal by construction. If
    /// this ever fails, something has grown a hand-written table again.
    #[test]
    fn every_front_end_exposes_exactly_the_registry() {
        let registry: Vec<String> = cadpole_ops::registry().iter().filter(|m| !m.hidden).map(|m| m.name.clone()).collect();
        // 31 visible operations: every rpc of CadpoleOps except `PostMessage`,
        // which is `(cadpole.op).hidden` because posting as the user is the user's
        // client's business and nobody else's (capability.rs). The last four
        // are the solve jobs.
        assert_eq!(registry.len(), 31, "{registry:?}");
        assert!(!registry.contains(&"PostMessage".to_owned()), "a hidden op must not reach a front-end's list");
        assert!(cadpole_ops::method("PostMessage").is_some(), "...but it is still callable by name");

        // HTTP: POST /api/ops/<Method>
        assert_eq!(crate::server::op_names(), registry);
        // ...and the Connect route, which is the same registry under a
        // protocol other people implement. Every rpc is a method of
        // `cadpole.v1.CadpoleOps` -- `PostMessage` included, because hiding an
        // operation from a listing is not hiding it from the schema, and a
        // generated client is built from the schema.
        for m in cadpole_ops::registry() {
            assert!(crate::connect::lookup(&m.name).is_some(), "{} is not reachable over Connect", m.name);
        }
        assert!(crate::connect::lookup("NoSuchOp").is_none());
        // CLI: cadpole op <Method>
        assert_eq!(crate::op_cli::op_names(), registry);
        // MCP: one tool per rpc, named in snake_case.
        let expected_tools: Vec<String> = cadpole_ops::registry().iter().filter(|m| !m.hidden).map(|m| m.snake.clone()).collect();
        assert_eq!(crate::mcp::tool_names(), expected_tools);
        // ...and snake_case is the only transformation between the two sets.
        let back: Vec<String> = crate::mcp::tool_names().iter().map(|t| cadpole_ops::method(t).unwrap().name.clone()).collect();
        assert_eq!(back, registry);
    }

    /// The verbs a person types are the same operations, not a second
    /// implementation: each maps onto a registry entry.
    #[test]
    fn the_ergonomic_verbs_map_onto_operations() {
        for (verb, op) in [
            ("eval", "Evaluate"),
            ("render", "Render"),
            ("sweep", "Sweep"),
            ("animate", "Animate"),
            ("export", "Export"),
            ("test", "RunTests"),
            ("run_tests", "RunTests"),
            ("check", "PrintCheck"),
            ("plate", "Plate"),
            ("inspect", "Inspect"),
            ("doc", "ApiDoc"),
            ("selection", "GetSelection"),
            ("listen", "WaitEvents"),
            ("explain-ref", "ExplainRef"),
            ("explain_ref", "ExplainRef"),
            ("edit-script", "EditScript"),
            ("edit_script", "EditScript"),
            ("attachment", "GetAttachment"),
            ("ack", "Ack"),
            ("assets-list", "ListAssets"),
            ("assets-add", "AddAsset"),
            ("assets-get", "GetAsset"),
        ] {
            assert_eq!(cadpole_ops::method(verb).map(|m| m.name.clone()), Some(op.to_owned()), "verb {verb}");
        }
    }
}
