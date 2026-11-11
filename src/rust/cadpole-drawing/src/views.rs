//! The views a sheet places: a plan cut at a height, a section
//! through a plane with the model beyond it, an elevation from a direction,
//! an axonometric from an azimuth and elevation.
//!
//! Each one is a list of drawing items in the view's own 2D frame, in model
//! millimetres, plus its bounds. The sheet wraps them in a [`Group`] whose
//! transform carries the scale and the placement and whose viewport clips
//! them, so a view never knows what page it is on and a page never knows
//! how a view was computed. Hidden-line removal is [`crate::view::project`]'s
//! — the same pass the `Drawing` rpc runs — over every body together, with
//! each body's lines tagged so a plan can put walls and site on layers of
//! their own.
//!
//! [`Group`]: crate::ir::Group

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::{Path, Sketch, P2};
use cadpole_core::tessellate::{Mesh, Topology};

use crate::ir::{layer, Dash, Drawing, HatchItem, Item, Pattern, Rgb, Stroke, Style};
use crate::painted::{BodyLabel, PaintStyle};
use crate::view::{
    append_body, clip_mesh, clip_topology, project_tagged, section_loops, BodyRanges, HalfSpace,
    Hidden, ProjectOptions, ViewBasis,
};

/// One body's share of a view: its tessellation and, when the script asked
/// for one, the layer its lines go on; for the colour look its colour, its
/// opacity and the text label it is a glyph of.
#[derive(Clone, Debug)]
pub struct ViewBody {
    pub name: String,
    pub mesh: Mesh,
    pub topo: Topology,
    pub layer: Option<String>,
    /// sRGB; `None` is a neutral grey.
    pub color: Option<Rgb>,
    pub opacity: f64,
    pub label: Option<BodyLabel>,
}

impl ViewBody {
    /// A body with no colour, fully opaque, no label.
    pub fn new(name: impl Into<String>, mesh: Mesh, topo: Topology) -> ViewBody {
        ViewBody { name: name.into(), mesh, topo, layer: None, color: None, opacity: 1.0, label: None }
    }

    /// Every length multiplied by `k` about the origin: a script drawn one
    /// unit to the foot drawn in millimetres (`model_unit`).
    pub fn scale(&mut self, k: f64) {
        for v in &mut self.mesh.positions {
            *v = (*v as f64 * k) as f32;
        }
        for e in &mut self.topo.edges {
            e.start = e.start * k;
            e.end = e.end * k;
            e.length *= k;
            for p in &mut e.polyline {
                *p = *p * k;
            }
        }
        if let Some(l) = &mut self.label {
            l.plane.origin = l.plane.origin * k;
            l.at = P2::new(l.at.x * k, l.at.y * k);
            l.cap_height *= k;
        }
    }
}

/// How a view looks. The default is the drafting look, which
/// every view drew before; a plate built with no other word is in colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// The drafting look: the cut outlined and hatched, what is beyond as
    /// hidden-line-removed strokes.
    #[default]
    Lines,
    /// The render's look as vectors: every surface the eye sees filled in
    /// its body's colour far to near, edges as fine lines, the cut filled
    /// in the colour of what it cuts, `text()` labels as text
    /// ([`crate::painted`]).
    Color,
}

impl Look {
    pub fn parse(s: &str) -> Option<Look> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "lines" | "line" | "drafting" => Some(Look::Lines),
            "color" | "colour" | "render" => Some(Look::Color),
            _ => None,
        }
    }
}

/// How a view is drawn.
#[derive(Clone, Debug)]
pub struct ViewOptions {
    pub hidden: Hidden,
    pub project: ProjectOptions,
    /// Hatch for the cut faces of a plan or section; `None` outlines them
    /// and leaves them empty.
    pub hatch: Option<Pattern>,
    pub hatch_scale: f64,
    /// The layer the cut outlines go on; the section layer by default.
    pub cut_layer: Option<String>,
    /// Chaining tolerance for the cut loops, in model millimetres.
    pub tolerance: f64,
    pub look: Look,
    /// Line weights and colours of the colour look.
    pub paint: PaintStyle,
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            hidden: Hidden::Omit,
            project: ProjectOptions::default(),
            hatch: Some(Pattern::ANSI31),
            hatch_scale: 1.0,
            cut_layer: None,
            tolerance: 0.01,
            look: Look::Lines,
            paint: PaintStyle::default(),
        }
    }
}

/// What a view computed: items in its 2D frame, in model millimetres.
#[derive(Clone, Debug, Default)]
pub struct ViewGeometry {
    pub items: Vec<Item>,
    pub bounds: Option<(P2, P2)>,
    pub notes: Vec<String>,
}

impl ViewGeometry {
    pub(crate) fn eat(&mut self, p: P2) {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return;
        }
        self.bounds = Some(match self.bounds {
            None => (p, p),
            Some((lo, hi)) => (
                P2::new(lo.x.min(p.x), lo.y.min(p.y)),
                P2::new(hi.x.max(p.x), hi.y.max(p.y)),
            ),
        });
    }
}

/// A view basis from a name: the drawing presets (`front`, `top`, `iso`,
/// …) or a compass direction (`north` is the north elevation, seen from
/// the north, with +Y as north).
pub fn basis_for(direction: &str) -> Option<ViewBasis> {
    let d = direction.trim().to_ascii_lowercase();
    let preset = match d.as_str() {
        "north" | "n" => "back",
        "south" | "s" => "front",
        "east" | "e" => "right",
        "west" | "w" => "left",
        "front" | "back" | "top" | "plan" | "bottom" | "left" | "right" | "iso" | "iso-back" => d.as_str(),
        _ => return None,
    };
    Some(ViewBasis::preset(preset))
}

/// The direction names [`basis_for`] accepts, for a diagnostic.
pub const DIRECTIONS: &[&str] = &[
    "north", "south", "east", "west", "front", "back", "left", "right", "top", "bottom", "iso",
    "iso-back",
];

/// An axonometric basis: azimuth from the front (south) view turning toward
/// +X, elevation above the ground, both in degrees — the renderer's own
/// `az=,el=` convention, so a sheet's axon matches a render's.
pub fn axon_basis(az_deg: f64, el_deg: f64) -> ViewBasis {
    let (az, el) = (az_deg.to_radians(), el_deg.to_radians());
    let dir = Vec3::new(az.sin() * el.cos(), -az.cos() * el.cos(), el.sin());
    ViewBasis::looking_from(dir, Vec3::Z)
}

/// The projected lines of `bodies` (clipped by `clips`, all of them) through
/// `basis`, as strokes on the standard layers or on each body's own, with
/// `shift` subtracted so the frame's origin lands where the caller wants it.
fn project_bodies(
    d: &mut Drawing,
    bodies: &[ViewBody],
    clips: &[HalfSpace],
    basis: &ViewBasis,
    shift: P2,
    opts: &ViewOptions,
    out: &mut ViewGeometry,
) {
    let mut mesh = Mesh::default();
    let mut topo = Topology::default();
    let mut ranges = BodyRanges::default();
    for b in bodies {
        let mut m = b.mesh.clone();
        let mut t = b.topo.clone();
        for hs in clips {
            m = clip_mesh(&m, hs);
            t = clip_topology(&t, hs);
        }
        append_body(&mut mesh, &mut topo, &mut ranges, &m, &t);
    }
    if mesh.indices.is_empty() {
        out.notes.push("the view is empty: nothing of the model is on the drawn side of the cut".to_owned());
        return;
    }
    let po = ProjectOptions {
        hidden: opts.hidden,
        ..opts.project.clone()
    };
    let p = project_tagged(&mesh, &topo, basis, &po, &ranges);
    let vis_default = d.layer(layer::VISIBLE);
    let hid = d.layer(layer::HIDDEN);
    let body_layers: Vec<usize> = bodies
        .iter()
        .map(|b| match &b.layer {
            Some(name) => d.layer(name),
            None => vis_default,
        })
        .collect();
    let place = |q: P2| P2::new(q.x - shift.x, q.y - shift.y);
    for (poly, body) in p.visible.iter().zip(&p.visible_body) {
        let pts: Vec<P2> = poly.iter().map(|q| place(*q)).collect();
        for q in &pts {
            out.eat(*q);
        }
        let l = body_layers.get(*body).copied().unwrap_or(vis_default);
        push_polyline(&mut out.items, Style::on(l), &pts);
    }
    if opts.hidden != Hidden::Omit {
        for poly in &p.hidden {
            let pts: Vec<P2> = poly.iter().map(|q| place(*q)).collect();
            for q in &pts {
                out.eat(*q);
            }
            let style = if opts.hidden == Hidden::Solid {
                Style::dashed(hid, Dash::Solid)
            } else {
                Style::on(hid)
            };
            push_polyline(&mut out.items, style, &pts);
        }
    }
}

fn push_polyline(items: &mut Vec<Item>, style: Style, pts: &[P2]) {
    if pts.len() < 2 {
        return;
    }
    let mut path = Path::new(pts[0]);
    for q in &pts[1..] {
        path = path.line_to(*q);
    }
    items.push(Item::Stroke(Stroke {
        style,
        name: String::new(),
        path,
        closed: false,
    }));
}

/// A section through `plane`: the cut faces of every body hatched and
/// outlined, and, when `beyond` is set, the model on the far side of the
/// plane (up to `depth` millimetres of it) projected behind them. The frame
/// is the plane's own: x along its `x_dir`, y along its `y_dir`, origin at
/// its origin, so a point of the cut has the coordinates a script would
/// compute for it.
pub fn section_view(
    d: &mut Drawing,
    bodies: &[ViewBody],
    plane: &Plane,
    depth: Option<f64>,
    beyond: bool,
    opts: &ViewOptions,
) -> ViewGeometry {
    let n = plane.normal.normalized();
    let basis = ViewBasis::looking_from(n, plane.y_dir());
    let shift = basis.project(plane.origin).0;
    if opts.look == Look::Color {
        let mut clips = vec![HalfSpace::new(plane.origin, -n)];
        if let Some(depth) = depth.filter(|v| v.is_finite() && *v > 0.0) {
            clips.push(HalfSpace::new(plane.origin - n * depth, n));
        }
        if !beyond {
            // The cut alone: nothing beyond is painted.
            clips.push(HalfSpace::new(plane.origin - n * 1e-9, n));
        }
        return crate::painted::paint(d, bodies, &clips, &basis, shift, Some(plane), opts);
    }
    let mut out = ViewGeometry::default();

    // What is beyond the cut, first, so the cut faces paint over it.
    if beyond {
        let mut clips = vec![HalfSpace::new(plane.origin, -n)];
        if let Some(depth) = depth.filter(|v| v.is_finite() && *v > 0.0) {
            clips.push(HalfSpace::new(plane.origin - n * depth, n));
        }
        project_bodies(d, bodies, &clips, &basis, shift, opts, &mut out);
    }

    let cut_layer = match &opts.cut_layer {
        Some(name) => d.layer(name),
        None => d.layer(layer::SECTION),
    };
    let hatch_layer = d.layer(layer::SECTION_HATCH);
    for b in bodies {
        let loops = section_loops(&b.mesh, plane, opts.tolerance);
        if loops.is_empty() {
            continue;
        }
        let sketch = loops.iter().fold(Sketch::new(*plane), |s, l| s.with_loop(l.clone()));
        match sketch.regions() {
            Ok(regions) => {
                for r in regions {
                    for l in std::iter::once(&r.outer).chain(&r.holes) {
                        for q in l.sample() {
                            out.eat(q);
                        }
                    }
                    if let Some(pattern) = &opts.hatch {
                        out.items.push(Item::Hatch(HatchItem {
                            style: Style::on(hatch_layer),
                            name: String::new(),
                            outer: r.outer.clone(),
                            holes: r.holes.clone(),
                            pattern: pattern.clone(),
                            scale: opts.hatch_scale,
                        }));
                    }
                    for l in std::iter::once(&r.outer).chain(&r.holes) {
                        let mut p = Path::new(l.segments[0].start());
                        for s in &l.segments {
                            p.segments.push(s.clone());
                        }
                        out.items.push(Item::Stroke(Stroke {
                            style: Style::on(cut_layer),
                            name: String::new(),
                            path: p,
                            closed: true,
                        }));
                    }
                }
            }
            Err(e) => {
                out.notes.push(format!(
                    "section of {}: the cut loops do not nest ({e}), so the cut face is outlined but not hatched",
                    b.name
                ));
                for l in &loops {
                    for q in l.sample() {
                        out.eat(q);
                    }
                    let mut p = Path::new(l.segments[0].start());
                    for s in &l.segments {
                        p.segments.push(s.clone());
                    }
                    out.items.push(Item::Stroke(Stroke {
                        style: Style::on(cut_layer),
                        name: String::new(),
                        path: p,
                        closed: true,
                    }));
                }
            }
        }
    }
    out
}

/// A plan: the model cut at `cut_z` and seen from above, x and y the
/// model's own. What is below the cut is projected; the cut walls are
/// outlined heavily and hatched.
pub fn plan_view(d: &mut Drawing, bodies: &[ViewBody], cut_z: f64, opts: &ViewOptions) -> ViewGeometry {
    section_view(d, bodies, &Plane::XY.offset(cut_z), None, true, opts)
}

/// An elevation or any named view: the whole model projected through
/// `basis`, hidden lines as `opts.hidden` says. The frame is the basis's
/// screen: for a south elevation x is the model's x and y its z.
pub fn elevation_view(d: &mut Drawing, bodies: &[ViewBody], basis: &ViewBasis, opts: &ViewOptions) -> ViewGeometry {
    if opts.look == Look::Color {
        return crate::painted::paint(d, bodies, &[], basis, P2::new(0.0, 0.0), None, opts);
    }
    let mut out = ViewGeometry::default();
    project_bodies(d, bodies, &[], basis, P2::new(0.0, 0.0), opts, &mut out);
    out
}

/// An axonometric from `az`/`el` degrees ([`axon_basis`]).
pub fn axon_view(d: &mut Drawing, bodies: &[ViewBody], az_deg: f64, el_deg: f64, opts: &ViewOptions) -> ViewGeometry {
    elevation_view(d, bodies, &axon_basis(az_deg, el_deg), opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Prim;
    use cadpole_core::sketch::Loop;

    fn body(name: &str, w: f64, dpt: f64, h: f64, z0: f64) -> ViewBody {
        let s = Sketch::new(Plane::XY.offset(z0)).with_loop(Loop::rect(P2::new(0.0, 0.0), w, dpt));
        let solids = cadpole_core::kernel::extrude(&s, h).expect("extrude");
        let (mesh, topo) = cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess");
        ViewBody::new(name, mesh, topo)
    }

    fn flat(d: &Drawing, items: &[Item]) -> Vec<Prim> {
        let mut dd = d.clone();
        dd.items = items.to_vec();
        dd.flatten()
    }

    #[test]
    fn a_plan_cuts_the_walls_and_shows_the_floor_below() {
        // A 40 x 20 x 30 tower: a plan at z = 15 cuts a 40 x 20 rectangle.
        let mut d = Drawing::new();
        let g = plan_view(&mut d, &[body("tower", 40.0, 20.0, 30.0, 0.0)], 15.0, &ViewOptions::default());
        let (lo, hi) = g.bounds.expect("bounds");
        assert!((hi.x - lo.x - 40.0).abs() < 1e-6 && (hi.y - lo.y - 20.0).abs() < 1e-6, "{lo:?} {hi:?}");
        let prims = flat(&d, &g.items);
        let cut = prims.iter().filter(|p| matches!(p, Prim::Path { style, closed: true, .. } if style.layer == layer::SECTION)).count();
        assert_eq!(cut, 1, "one cut outline");
        let hatch = prims.iter().filter(|p| p.style().layer == layer::SECTION_HATCH).count();
        assert!(hatch > 5, "the cut is hatched: {hatch}");
        // Nothing projected from above the cut: every vertex of every
        // stroke is within the footprint.
        for p in &prims {
            if let Prim::Path { path, .. } = p {
                for q in crate::ir::sample_path(path, 0.1) {
                    assert!(q.x >= -20.0 - 1e-6 && q.x <= 20.0 + 1e-6);
                }
            }
        }
    }

    #[test]
    fn a_section_keeps_only_what_is_beyond_the_plane_and_a_depth_limits_it() {
        // Two boxes along y: one at y in [-10, 10], a taller one at y in
        // [30, 50] whose top shows above the near one.
        let near = body("near", 20.0, 20.0, 10.0, 0.0);
        let mut far = body("far", 20.0, 20.0, 30.0, 0.0);
        for p in far.mesh.positions.chunks_exact_mut(3) {
            p[1] += 40.0;
        }
        for e in &mut far.topo.edges {
            e.start.y += 40.0;
            e.end.y += 40.0;
            for p in &mut e.polyline {
                p.y += 40.0;
            }
        }
        // Cut the near box at y = 0 looking toward +y (the plane's normal
        // is -y, the eye on the -y side, +y beyond the cut).
        let plane = Plane::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::X).unwrap();
        let mut d = Drawing::new();
        let opts = ViewOptions::default();
        let all = section_view(&mut d, &[near.clone(), far.clone()], &plane, None, true, &opts);
        let limited = section_view(&mut d, &[near.clone(), far.clone()], &plane, Some(20.0), true, &opts);
        let strokes = |g: &ViewGeometry| {
            flat(&d, &g.items)
                .iter()
                .filter(|p| matches!(p, Prim::Path { style, .. } if style.layer == layer::VISIBLE))
                .count()
        };
        assert!(strokes(&all) > strokes(&limited), "the far box is beyond 20 mm: {} vs {}", strokes(&all), strokes(&limited));
        // The cut face is the near box's 20 x 10 section, in the plane's
        // frame (x along X, y up).
        let cut = flat(&d, &all.items).iter().filter(|p| p.style().layer == layer::SECTION).count();
        assert_eq!(cut, 1);
        let (lo, hi) = limited.bounds.unwrap();
        assert!(lo.y >= -1e-6 && hi.y <= 10.0 + 1e-6, "{lo:?} {hi:?}");
        let (_, hi) = all.bounds.unwrap();
        assert!((hi.y - 30.0).abs() < 1e-6, "the far box's top shows at 30: {hi:?}");
    }

    #[test]
    fn directions_and_axon_bases_read_the_compass() {
        let north = basis_for("north").unwrap();
        assert!((north.toward_eye.y - 1.0).abs() < 1e-9, "seen from the north");
        assert!(basis_for("sideways").is_none());
        let iso = axon_basis(45.0, 29.5);
        assert!(iso.toward_eye.x > 0.0 && iso.toward_eye.y < 0.0 && iso.toward_eye.z > 0.0);
        let mut d = Drawing::new();
        let g = axon_view(&mut d, &[body("b", 10.0, 10.0, 10.0, 0.0)], 45.0, 30.0, &ViewOptions::default());
        assert!(g.bounds.is_some());
        let n = flat(&d, &g.items).len();
        assert!((9..=9).contains(&n), "nine visible edges of a box from a corner, hidden omitted: {n}");
    }
}
