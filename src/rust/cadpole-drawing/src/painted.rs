//! The colour look: a plan or a section drawn the way `cadpole render
//! --section` shows it, as vectors.
//!
//! The line look ([`crate::views`]) is a drafting convention: the cut
//! outlined and hatched, everything beyond as hidden-line-removed strokes.
//! A design booklet's plans are pictures of the model instead: every
//! surface the eye sees filled in its material's colour, the cut faces
//! filled in the colour of what they cut, edges as fine lines, and the
//! room names the script set in the model written as text. This module
//! draws that.
//!
//! **Visibility is the painter's.** Each triangle facing the eye is a
//! filled polygon in its body's colour, shaded a little by how squarely it
//! faces the eye, and the triangles are painted far to near, so a nearer
//! surface covers a farther one. Runs of neighbouring triangles of one body
//! and one shade are written as one fill with many sub-paths, which every
//! PDF and SVG rasteriser covers without the hairline seams that separate
//! fills leave. The edges are the topology's creases (the corners of the
//! solids, as the line look takes them) and the silhouettes of smooth
//! surfaces, each painted at its own depth so a nearer surface covers it
//! too; the cut faces and their outlines go on last, on top of everything,
//! because nothing kept is nearer than the cut. Sorting starts from each
//! piece's centroid depth, which is exact for the horizontal surfaces a plan
//! is made of and for the walls a section looks at, but not for a face at a
//! slant to the view: a long triangle of a canted bay face can have its
//! centroid behind a small triangle of the wall it stands in front of, and
//! painted first, the wall shows through it in shards. So wherever two
//! triangles, or an edge and a triangle, overlap on the paper, the one in
//! front at a point of the overlap is painted after the other
//! ([`depth_ordered`]); the centroid order only breaks ties and the rare
//! cycle, which a z-buffer would resolve and a vector file cannot.
//!
//! **Text is the script's.** A label the script made with
//! `extrude(text("Kitchen", ...), h)` is not drawn as its glyph outlines:
//! its string, its face, its size and its frame are in the context
//! ([`cadpole_core::context::TextLabel`]), so it is written as one text item
//! the size it is in the model, at the depth of its top face (a label under
//! a roof is covered by the roof), and the PDF carries it as real text.

use std::collections::HashMap;

use cadpole_core::context::{BodyRecord, Context};
use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::{Loop, Path, Segment, Sketch, P2};
use cadpole_core::tessellate::{Mesh, Topology};

use crate::ir::{layer, Anchor, Drawing, FontRef, HAlign, HatchItem, Item, Pattern, Rgb, Stroke, Style, TextItem, VAlign};
use crate::view::{clip_mesh, clip_topology, section_loops, HalfSpace, ViewBasis};
use crate::views::{ViewBody, ViewGeometry, ViewOptions};

/// The layer the colour look's surface fills go on.
pub const FILL_LAYER: &str = "A-VIEW-FILL";

/// A text label a body is a glyph of: the string and how it was
/// set, from the `text()` call that made it.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyLabel {
    /// Every glyph body of one label shares this: the feature that made
    /// them. The label is written once per group.
    pub group: String,
    pub text: String,
    /// Cap height, model mm.
    pub cap_height: f64,
    pub anchor: Anchor,
    pub bold: bool,
    /// The plane it was set on and its anchor in that plane.
    pub plane: Plane,
    pub at: P2,
}

/// The display colour of a body: its material's base colour, which the
/// context keeps in linear RGB, as sRGB.
pub fn srgb(linear: [f64; 3]) -> Rgb {
    let f = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgb(f(linear[0]), f(linear[1]), f(linear[2]))
}

/// What the colour look needs of a body beyond its mesh: its colour, its
/// opacity and, for a glyph of a `text()` label set in the bundled face,
/// the label.
pub fn paint_of(ctx: &Context, b: &BodyRecord) -> (Option<Rgb>, f64, Option<BodyLabel>) {
    let color = Some(srgb(b.material.base_color));
    let label = ctx.text_label_of(b).and_then(|(group, l)| {
        if !(l.plain && (l.font == "builtin" || l.font == "sans" || l.font == "default")) {
            return None;
        }
        let h = match l.align.as_str() {
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            _ => HAlign::Left,
        };
        let v = match l.baseline.as_str() {
            "middle" => VAlign::Middle,
            "top" => VAlign::Top,
            _ => VAlign::Baseline,
        };
        // The writers stack lines 1.2 em apart, as text() does by default.
        if l.text.contains('\n') && (l.line_height - 1.2).abs() > 1e-9 {
            return None;
        }
        Some(BodyLabel {
            group: group.0.clone(),
            text: l.text.clone(),
            cap_height: l.cap_height,
            anchor: Anchor::new(h, v),
            // The bundled face comes in two weights in a PDF; semibold and
            // up is its bold.
            bold: l.weight >= 550.0,
            plane: l.plane,
            at: l.at,
        })
    });
    (color, b.material.opacity, label)
}

/// How the colour look draws, in paper millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintStyle {
    /// The creases and silhouettes of what is seen beyond the cut, in
    /// `edge_color` or, with none, a darker shade of the body's colour, so
    /// a small or thin body -- a dashed level line, a mullion -- reads in
    /// its own hue rather than as a grey outline.
    pub edge_weight_mm: f64,
    pub edge_color: Option<Rgb>,
    /// The outline of every cut face, in `cut_color` or, with none, a
    /// darker shade of the face's own fill, so a thin cut piece -- a level
    /// line, a window's glass -- keeps its hue under its outline.
    pub cut_weight_mm: f64,
    pub cut_color: Option<Rgb>,
    /// A topology edge at or above this dihedral is drawn.
    pub crease_degrees: f64,
    /// Below this opacity a body's surfaces are not filled -- glass, a
    /// ghosted envelope -- and only its edges and its cut are drawn.
    pub fill_opacity: f64,
    /// How far a surface seen beyond the cut is lightened toward white, so
    /// the cut reads as the nearest thing (0 keeps the material's colour).
    pub seen_lighten: f64,
}

impl Default for PaintStyle {
    fn default() -> Self {
        PaintStyle {
            edge_weight_mm: 0.09,
            edge_color: None,
            cut_weight_mm: 0.2,
            cut_color: None,
            crease_degrees: 15.0,
            fill_opacity: 0.6,
            seen_lighten: 0.2,
        }
    }
}

/// One thing to paint, at a depth.
enum Piece {
    /// A front-facing triangle of body `body`, its projected corners, their
    /// depths and its shaded colour.
    Tri { body: usize, pts: [P2; 3], depths: [f64; 3], color: Rgb },
    /// A visible edge segment and its ends' depths.
    Edge { body: usize, a: P2, b: P2, depths: [f64; 2] },
    /// A label, written once.
    Label { body: usize, label: usize },
}

/// Paint `bodies` as the colour look sees them through `basis`: what is on
/// the kept side of every clip, filled and edged far to near, the cut by
/// `cut` (when there is one) filled on top, `shift` subtracted from every
/// point so the frame's origin lands where the caller wants it.
pub(crate) fn paint(
    d: &mut Drawing,
    bodies: &[ViewBody],
    clips: &[HalfSpace],
    basis: &ViewBasis,
    shift: P2,
    cut: Option<&Plane>,
    opts: &ViewOptions,
) -> ViewGeometry {
    let style = &opts.paint;
    let mut out = ViewGeometry::default();
    let place = |p: Vec3| {
        let (q, depth) = basis.project(p);
        (P2::new(q.x - shift.x, q.y - shift.y), depth)
    };
    let diag = bodies
        .iter()
        .filter_map(|b| b.mesh.bbox())
        .fold(None::<(Vec3, Vec3)>, |acc, (lo, hi)| match acc {
            None => Some((lo, hi)),
            Some((a, b)) => Some((
                Vec3::new(a.x.min(lo.x), a.y.min(lo.y), a.z.min(lo.z)),
                Vec3::new(b.x.max(hi.x), b.y.max(hi.y), b.z.max(hi.z)),
            )),
        })
        .map(|(lo, hi)| (hi - lo).length())
        .unwrap_or(1.0)
        .max(1e-9);
    let eps = diag * 1e-7;

    let mut pieces: Vec<(f64, usize, Piece)> = Vec::new();
    let mut labels: Vec<(usize, &BodyLabel)> = Vec::new();
    let mut seen_groups: Vec<&str> = Vec::new();
    let push = |pieces: &mut Vec<(f64, usize, Piece)>, depth: f64, p: Piece| {
        let n = pieces.len();
        pieces.push((depth, n, p));
    };

    for (bi, b) in bodies.iter().enumerate() {
        let mut mesh = b.mesh.clone();
        let mut topo = b.topo.clone();
        for hs in clips {
            mesh = clip_mesh(&mesh, hs);
            topo = clip_topology(&topo, hs);
        }
        if mesh.indices.is_empty() {
            continue;
        }
        if let Some(label) = &b.label {
            // A glyph: the label goes in once, at the depth of the nearest
            // glyph's top, when its plane faces the eye; the glyph's own
            // surfaces are not drawn.
            let facing = label.plane.normal.normalized().dot(basis.toward_eye);
            if facing > 0.99 && !seen_groups.contains(&label.group.as_str()) {
                seen_groups.push(&label.group);
                let top = bodies
                    .iter()
                    .filter(|o| o.label.as_ref().is_some_and(|l| l.group == label.group))
                    .filter_map(|o| o.mesh.bbox())
                    .flat_map(|(lo, hi)| corners(lo, hi))
                    .map(|p| basis.project(p).1)
                    .fold(f64::MIN, f64::max);
                labels.push((bi, label));
                let li = labels.len() - 1;
                push(&mut pieces, top + 2.0 * eps, Piece::Label { body: bi, label: li });
            }
            continue;
        }
        let color = b.color.unwrap_or(Rgb(184, 189, 199));
        if b.opacity >= style.fill_opacity {
            for t in mesh.indices.chunks_exact(3) {
                let p = [vtx(&mesh, t[0]), vtx(&mesh, t[1]), vtx(&mesh, t[2])];
                let n = (p[1] - p[0]).cross(p[2] - p[0]);
                let len = n.length();
                if len <= 1e-18 {
                    continue;
                }
                let facing = n.dot(basis.toward_eye) / len;
                if facing <= 0.02 {
                    continue;
                }
                let (q0, d0) = place(p[0]);
                let (q1, d1) = place(p[1]);
                let (q2, d2) = place(p[2]);
                push(
                    &mut pieces,
                    (d0 + d1 + d2) / 3.0,
                    Piece::Tri { body: bi, pts: [q0, q1, q2], depths: [d0, d1, d2], color: mix_white(shade(color, facing), style.seen_lighten) },
                );
            }
        }
        // The creases the topology knows, as far as the clips left them.
        for e in &topo.edges {
            if !(e.dihedral_degrees < 0.0 || e.dihedral_degrees >= style.crease_degrees) {
                continue;
            }
            let poly: Vec<Vec3> = if e.polyline.len() >= 2 { e.polyline.clone() } else { vec![e.start, e.end] };
            for w in poly.windows(2) {
                // An edge lying in a clip plane is where the cut is: the cut
                // outline draws it.
                if clips.iter().any(|hs| on_plane(hs, w[0], eps * 10.0) && on_plane(hs, w[1], eps * 10.0)) {
                    continue;
                }
                let (a, da) = place(w[0]);
                let (bq, db) = place(w[1]);
                if (a.x - bq.x).abs() + (a.y - bq.y).abs() < eps {
                    continue;
                }
                push(&mut pieces, (da + db) / 2.0 + eps, Piece::Edge { body: bi, a, b: bq, depths: [da, db] });
            }
        }
        // Silhouettes of smooth surfaces: a mesh edge inside one face whose
        // two triangles face opposite ways.
        for (a, bq, depth) in silhouettes(&mesh, basis, eps) {
            let (pa, da) = place(a);
            let (pb, db) = place(bq);
            push(&mut pieces, depth + eps, Piece::Edge { body: bi, a: pa, b: pb, depths: [da, db] });
        }
    }

    pieces.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let pieces = depth_ordered(pieces, diag * 1e-6);

    // Where each label lands, and whether anything nearer covers it
    // entirely: a label under a floor is not drawn, so it is not text a
    // reader finds either (the scripts lay every level's room names and
    // let the floors above hide them, as a render does).
    let frame = |l: &BodyLabel| {
        let at3 = l.plane.to_world(l.at.x, l.at.y);
        let (a, _) = place(at3);
        let (ax, _) = place(at3 + l.plane.x_dir.normalized());
        let rotation = (ax.y - a.y).atan2(ax.x - a.x).to_degrees();
        (a, if rotation.abs() < 1e-9 { 0.0 } else { rotation })
    };
    let mut hidden: Vec<bool> = vec![false; labels.len()];
    for (k, (_, _, piece)) in pieces.iter().enumerate() {
        let Piece::Label { label, .. } = piece else { continue };
        let l = labels[*label].1;
        let (a, rot) = frame(l);
        let mut samples = label_samples(l, a, rot);
        let (lo, hi) = samples.iter().fold((P2::new(f64::MAX, f64::MAX), P2::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
            (P2::new(lo.x.min(p.x), lo.y.min(p.y)), P2::new(hi.x.max(p.x), hi.y.max(p.y)))
        });
        for (_, _, later) in &pieces[k + 1..] {
            let Piece::Tri { pts, .. } = later else { continue };
            let (tlo, thi) = (
                P2::new(pts[0].x.min(pts[1].x).min(pts[2].x), pts[0].y.min(pts[1].y).min(pts[2].y)),
                P2::new(pts[0].x.max(pts[1].x).max(pts[2].x), pts[0].y.max(pts[1].y).max(pts[2].y)),
            );
            if thi.x < lo.x || tlo.x > hi.x || thi.y < lo.y || tlo.y > hi.y {
                continue;
            }
            samples.retain(|p| !in_triangle(*p, pts));
            if samples.is_empty() {
                break;
            }
        }
        hidden[*label] = samples.is_empty();
    }
    let fill_layer = d.layer(FILL_LAYER);
    let edge_layer_default = d.layer(layer::VISIBLE);
    let text_layer = d.layer(layer::TEXT);
    let body_layers: Vec<usize> = bodies
        .iter()
        .map(|b| match &b.layer {
            Some(name) => d.layer(name),
            None => edge_layer_default,
        })
        .collect();
    let edge_style = |body: usize, l: usize| Style {
        layer: l,
        weight_mm: Some(style.edge_weight_mm),
        dash: None,
        color: Some(style.edge_color.unwrap_or_else(|| darken(mix_white(bodies[body].color.unwrap_or(Rgb(184, 189, 199)), 0.1), 0.62))),
    };

    // Runs: consecutive triangles of one body and one shade become one
    // fill, consecutive edges of one body one stroke where they chain.
    let mut tri_run: Option<(usize, Rgb, Vec<Loop>)> = None;
    let mut edge_run: Option<(usize, Vec<P2>)> = None;
    let flush_tris = |run: &mut Option<(usize, Rgb, Vec<Loop>)>, out: &mut ViewGeometry| {
        if let Some((_, color, mut loops)) = run.take() {
            if loops.is_empty() {
                return;
            }
            let outer = loops.remove(0);
            out.items.push(Item::Hatch(HatchItem {
                style: Style { layer: fill_layer, weight_mm: None, dash: None, color: Some(color) },
                name: String::new(),
                outer,
                holes: loops,
                pattern: Pattern::Solid,
                scale: 1.0,
            }));
        }
    };
    let flush_edges = |run: &mut Option<(usize, Vec<P2>)>, out: &mut ViewGeometry| {
        if let Some((body, pts)) = run.take() {
            if pts.len() >= 2 {
                let mut path = Path::new(pts[0]);
                for q in &pts[1..] {
                    path = path.line_to(*q);
                }
                out.items.push(Item::Stroke(Stroke {
                    style: edge_style(body, body_layers[body]),
                    name: String::new(),
                    path,
                    closed: false,
                }));
            }
        }
    };
    for (_, _, piece) in &pieces {
        match piece {
            Piece::Tri { body, pts, color, .. } => {
                flush_edges(&mut edge_run, &mut out);
                for q in pts {
                    out.eat(*q);
                }
                let l = Loop {
                    segments: vec![
                        Segment::Line { a: pts[0], b: pts[1] },
                        Segment::Line { a: pts[1], b: pts[2] },
                        Segment::Line { a: pts[2], b: pts[0] },
                    ],
                };
                match &mut tri_run {
                    Some((rb, rc, loops)) if rb == body && rc == color && loops.len() < 4000 => loops.push(l),
                    _ => {
                        flush_tris(&mut tri_run, &mut out);
                        tri_run = Some((*body, *color, vec![l]));
                    }
                }
            }
            Piece::Edge { body, a, b, .. } => {
                flush_tris(&mut tri_run, &mut out);
                out.eat(*a);
                out.eat(*b);
                match &mut edge_run {
                    Some((rb, pts)) if rb == body && pts.last().is_some_and(|l| (l.x - a.x).abs() + (l.y - a.y).abs() < eps) => {
                        pts.push(*b)
                    }
                    _ => {
                        flush_edges(&mut edge_run, &mut out);
                        edge_run = Some((*body, vec![*a, *b]));
                    }
                }
            }
            Piece::Label { body, label } => {
                if hidden[*label] {
                    continue;
                }
                flush_tris(&mut tri_run, &mut out);
                flush_edges(&mut edge_run, &mut out);
                let l = labels[*label].1;
                let (a, rotation) = frame(l);
                out.eat(a);
                out.items.push(Item::Text(TextItem {
                    style: Style { layer: text_layer, weight_mm: None, dash: None, color: bodies[*body].color },
                    at: a,
                    text: l.text.clone(),
                    height_mm: l.cap_height,
                    rotation_deg: rotation,
                    anchor: l.anchor,
                    font: if l.bold { FontRef::BuiltinBold } else { FontRef::Builtin },
                    model_height: true,
                }));
            }
        }
    }
    flush_tris(&mut tri_run, &mut out);
    flush_edges(&mut edge_run, &mut out);

    // The cut, on top: each body's section filled in its colour (lightened
    // for a translucent body) and outlined.
    if let Some(plane) = cut {
        let cut_fill = d.layer(layer::SECTION_HATCH);
        let cut_line = d.layer(layer::SECTION);
        for b in bodies {
            if b.label.is_some() {
                continue;
            }
            let loops = section_loops(&b.mesh, plane, opts.tolerance);
            if loops.is_empty() {
                continue;
            }
            let base = b.color.unwrap_or(Rgb(184, 189, 199));
            let color = if b.opacity < style.fill_opacity { mix_white(base, 0.6) } else { base };
            let sketch = loops.iter().fold(Sketch::new(*plane), |s, l| s.with_loop(l.clone()));
            let regions = sketch.regions().ok();
            if let Some(regions) = &regions {
                for r in regions {
                    out.items.push(Item::Hatch(HatchItem {
                        style: Style { layer: cut_fill, weight_mm: None, dash: None, color: Some(color) },
                        name: String::new(),
                        outer: r.outer.clone(),
                        holes: r.holes.clone(),
                        pattern: Pattern::Solid,
                        scale: 1.0,
                    }));
                }
            } else {
                out.notes.push(format!(
                    "section of {}: the cut loops do not nest, so the cut face is outlined but not filled",
                    b.name
                ));
            }
            for l in &loops {
                for q in l.sample() {
                    out.eat(q);
                }
                let mut p = Path::new(l.segments[0].start());
                p.segments.extend(l.segments.iter().cloned());
                out.items.push(Item::Stroke(Stroke {
                    style: Style {
                        layer: cut_line,
                        weight_mm: Some(style.cut_weight_mm),
                        dash: None,
                        color: Some(style.cut_color.unwrap_or_else(|| darken(color, 0.5))),
                    },
                    name: String::new(),
                    path: p,
                    closed: true,
                }));
            }
        }
    }
    if out.items.is_empty() {
        out.notes.push("the view is empty: nothing of the model is on the drawn side of the cut".to_owned());
    }
    out
}

/// Five points of a label's ink box -- its centre and its corners pulled a
/// sixth of the way in -- in the view's frame: a label every one of which
/// is under something nearer is hidden.
/// `pieces`, sorted by centroid depth, reordered so that wherever two
/// triangles overlap on the paper, or an edge crosses a triangle, the one
/// nearer the eye at a point of the overlap is painted later. A topological
/// sort over those constraints that always takes the ready piece earliest
/// in the centroid order, so with no overlaps the order is unchanged; a
/// cycle (three slivers that each cover the next) is broken at the
/// earliest piece left. `tol` is the depth difference that counts: two
/// surfaces closer than it are one surface.
fn depth_ordered(pieces: Vec<(f64, usize, Piece)>, tol: f64) -> Vec<(f64, usize, Piece)> {
    use std::collections::BTreeSet;
    let n = pieces.len();
    // Screen boxes of the pieces that take part: triangles and edges.
    let boxes: Vec<Option<(P2, P2)>> = pieces
        .iter()
        .map(|(_, _, p)| match p {
            Piece::Tri { pts, .. } => Some(bbox2(pts)),
            Piece::Edge { a, b, .. } => Some(bbox2(&[*a, *b])),
            Piece::Label { .. } => None,
        })
        .collect();
    let all = boxes.iter().flatten().fold(None::<(P2, P2)>, |acc, (lo, hi)| match acc {
        None => Some((*lo, *hi)),
        Some((a, b)) => Some((P2::new(a.x.min(lo.x), a.y.min(lo.y)), P2::new(b.x.max(hi.x), b.y.max(hi.y)))),
    });
    let Some((lo, hi)) = all else { return pieces };
    // A uniform grid over the paper, about four pieces to a cell.
    let count = boxes.iter().flatten().count().max(1);
    let side = ((count as f64 / 4.0).sqrt().ceil() as usize).clamp(1, 1024);
    let (w, h) = ((hi.x - lo.x).max(1e-12), (hi.y - lo.y).max(1e-12));
    let cell = |p: P2| -> (usize, usize) {
        let cx = (((p.x - lo.x) / w) * side as f64).floor().clamp(0.0, (side - 1) as f64) as usize;
        let cy = (((p.y - lo.y) / h) * side as f64).floor().clamp(0.0, (side - 1) as f64) as usize;
        (cx, cy)
    };
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); side * side];
    for (i, b) in boxes.iter().enumerate() {
        let Some((blo, bhi)) = b else { continue };
        let ((x0, y0), (x1, y1)) = (cell(*blo), cell(*bhi));
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                grid[cy * side + cx].push(i as u32);
            }
        }
    }
    // Constraints: `after[i]` are painted after piece i.
    let mut after: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut indegree = vec![0u32; n];
    // A pair is examined once, in the first cell (row-major) both boxes
    // share; past this many candidate pairs the centroid order stands.
    const BUDGET: usize = 50_000_000;
    let mut examined = 0usize;
    'cells: for (c, members) in grid.iter().enumerate() {
        let (cx, cy) = (c % side, c / side);
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                examined += 1;
                if examined > BUDGET {
                    break 'cells;
                }
                let (bi, bj) = (boxes[i as usize].unwrap(), boxes[j as usize].unwrap());
                let slo = P2::new(bi.0.x.max(bj.0.x), bi.0.y.max(bj.0.y));
                let shi = P2::new(bi.1.x.min(bj.1.x), bi.1.y.min(bj.1.y));
                if slo.x > shi.x || slo.y > shi.y || cell(slo) != (cx, cy) {
                    continue;
                }
                let (pi, pj) = (&pieces[i as usize].2, &pieces[j as usize].2);
                let Some(nearer) = nearer_where_they_overlap(pi, pj, tol) else { continue };
                let (far, near) = if nearer == 0 { (j, i) } else { (i, j) };
                after[far as usize].push(near);
                indegree[near as usize] += 1;
            }
        }
    }
    // Kahn's sort, always taking the earliest ready piece (the input is in
    // centroid order, so the index is the priority).
    let mut ready: BTreeSet<u32> = (0..n as u32).filter(|&i| indegree[i as usize] == 0).collect();
    let mut left: BTreeSet<u32> = (0..n as u32).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(&first_left) = left.iter().next() {
        let i = match ready.pop_first() {
            Some(i) => i,
            // A cycle: break it at the earliest piece left.
            None => first_left,
        };
        if !left.remove(&i) {
            continue;
        }
        order.push(i);
        for &j in &after[i as usize] {
            let d = &mut indegree[j as usize];
            *d = d.saturating_sub(1);
            if *d == 0 && left.contains(&j) {
                ready.insert(j);
            }
        }
    }
    let mut slots: Vec<Option<(f64, usize, Piece)>> = pieces.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i as usize].take()).collect()
}

fn bbox2(pts: &[P2]) -> (P2, P2) {
    pts.iter().fold((P2::new(f64::MAX, f64::MAX), P2::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (P2::new(lo.x.min(p.x), lo.y.min(p.y)), P2::new(hi.x.max(p.x), hi.y.max(p.y)))
    })
}

/// Which of two pieces is nearer the eye where they overlap on the paper:
/// `Some(0)` the first, `Some(1)` the second, `None` when they do not
/// overlap (sharing an edge is not overlapping), are not a triangle and
/// something, or are the same surface there to within `tol`. An edge lying
/// on a triangle counts as in front of it, since it is drawn on it.
fn nearer_where_they_overlap(a: &Piece, b: &Piece, tol: f64) -> Option<usize> {
    match (a, b) {
        (Piece::Tri { pts: pa, depths: da, .. }, Piece::Tri { pts: pb, depths: db, .. }) => {
            let at = overlap_point(pa, pb)?;
            let (za, zb) = (plane_depth(pa, da, at)?, plane_depth(pb, db, at)?);
            if (za - zb).abs() <= tol {
                None
            } else if za > zb {
                Some(0)
            } else {
                Some(1)
            }
        }
        (Piece::Edge { a: ea, b: eb, depths, .. }, Piece::Tri { pts, depths: dt, .. }) => {
            let (at, ze) = segment_inside(*ea, *eb, *depths, pts)?;
            let zt = plane_depth(pts, dt, at)?;
            Some(if ze >= zt - tol { 0 } else { 1 })
        }
        (Piece::Tri { .. }, Piece::Edge { .. }) => nearer_where_they_overlap(b, a, tol).map(|k| 1 - k),
        _ => None,
    }
}

/// Twice the signed area of a 2-D triangle.
fn area2(t: &[P2; 3]) -> f64 {
    (t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y)
}

/// The depth at `p` of the plane through a triangle's three projected
/// corners and their depths; `None` for a triangle seen edge-on.
fn plane_depth(t: &[P2; 3], d: &[f64; 3], p: P2) -> Option<f64> {
    let a = area2(t);
    if a.abs() <= 1e-18 {
        return None;
    }
    let w1 = ((p.x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (p.y - t[0].y)) / a;
    let w2 = ((t[1].x - t[0].x) * (p.y - t[0].y) - (p.x - t[0].x) * (t[1].y - t[0].y)) / a;
    Some(d[0] + (d[1] - d[0]) * w1 + (d[2] - d[0]) * w2)
}

/// A point inside both triangles' interiors, when their overlap has an
/// area: the centroid of one clipped by the other. Two triangles sharing
/// an edge or a corner clip to a sliver of no area and answer `None`.
fn overlap_point(a: &[P2; 3], b: &[P2; 3]) -> Option<P2> {
    let (sa, sb) = (area2(a), area2(b));
    if sa.abs() <= 1e-18 || sb.abs() <= 1e-18 {
        return None;
    }
    let mut poly: Vec<P2> = a.to_vec();
    for k in 0..3 {
        let (p, q) = (b[k], b[(k + 1) % 3]);
        // Inside is the side of each edge the triangle's own area is on.
        let side = |x: P2| ((q.x - p.x) * (x.y - p.y) - (q.y - p.y) * (x.x - p.x)) * sb.signum();
        let mut next = Vec::with_capacity(poly.len() + 2);
        for m in 0..poly.len() {
            let (u, v) = (poly[m], poly[(m + 1) % poly.len()]);
            let (su, sv) = (side(u), side(v));
            if su >= 0.0 {
                next.push(u);
            }
            if (su >= 0.0) != (sv >= 0.0) {
                let t = su / (su - sv);
                next.push(P2::new(u.x + (v.x - u.x) * t, u.y + (v.y - u.y) * t));
            }
        }
        poly = next;
        if poly.len() < 3 {
            return None;
        }
    }
    // The clipped polygon's area and centroid.
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for m in 0..poly.len() {
        let (u, v) = (poly[m], poly[(m + 1) % poly.len()]);
        let c = u.x * v.y - v.x * u.y;
        area += c;
        cx += (u.x + v.x) * c;
        cy += (u.y + v.y) * c;
    }
    // An overlap thinner than a millionth of the smaller triangle is a
    // shared edge seen through rounding.
    if area.abs() <= 1e-6 * sa.abs().min(sb.abs()) {
        return None;
    }
    Some(P2::new(cx / (3.0 * area), cy / (3.0 * area)))
}

/// The middle of the stretch of segment `a`–`b` strictly inside triangle
/// `t`, and the segment's depth there; `None` when no stretch of length is
/// inside (an edge along the triangle's border, or outside it).
fn segment_inside(a: P2, b: P2, d: [f64; 2], t: &[P2; 3]) -> Option<(P2, f64)> {
    let s = area2(t);
    if s.abs() <= 1e-18 {
        return None;
    }
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let scale = s.abs().sqrt();
    for k in 0..3 {
        let (p, q) = (t[k], t[(k + 1) % 3]);
        let side = |x: P2| ((q.x - p.x) * (x.y - p.y) - (q.y - p.y) * (x.x - p.x)) * s.signum();
        // Strictly inside, by a hair: an edge on the border is not inside.
        let margin = 1e-9 * scale * (q.x - p.x).hypot(q.y - p.y);
        let (sa, sb) = (side(a) - margin, side(b) - margin);
        if sa < 0.0 && sb < 0.0 {
            return None;
        }
        if sa < 0.0 || sb < 0.0 {
            let u = sa / (sa - sb);
            if sa < 0.0 {
                t0 = t0.max(u);
            } else {
                t1 = t1.min(u);
            }
        }
    }
    let len = (b.x - a.x).hypot(b.y - a.y);
    if t1 - t0 <= 1e-9 || len * (t1 - t0) <= 1e-9 * scale {
        return None;
    }
    let u = (t0 + t1) / 2.0;
    Some((P2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u), d[0] + (d[1] - d[0]) * u))
}

fn label_samples(l: &BodyLabel, at: P2, rotation_deg: f64) -> Vec<P2> {
    let font = if l.bold { FontRef::BuiltinBold } else { FontRef::Builtin };
    let cap = l.cap_height;
    let w = crate::text::width_in(&l.text, cap, &font);
    let n = l.text.split('\n').count() as f64;
    let step = 1.2 * crate::text::em_for_cap_height_in(cap, &font);
    let shift = match l.anchor.v {
        VAlign::Baseline | VAlign::Bottom => 0.0,
        VAlign::Middle => (-cap + (n - 1.0) * step) / 2.0,
        VAlign::Top => -cap,
    };
    let (y0, y1) = (shift - (n - 1.0) * step, shift + cap);
    let x0 = match l.anchor.h {
        HAlign::Left => 0.0,
        HAlign::Center => -w / 2.0,
        HAlign::Right => -w,
    };
    let (x1, cx, cy) = (x0 + w, x0 + w / 2.0, (y0 + y1) / 2.0);
    let inset = |x: f64, y: f64| (x + (cx - x) / 6.0, y + (cy - y) / 6.0);
    let (c, s) = (rotation_deg.to_radians().cos(), rotation_deg.to_radians().sin());
    [(cx, cy), inset(x0, y0), inset(x1, y0), inset(x1, y1), inset(x0, y1)]
        .iter()
        .map(|(x, y)| P2::new(at.x + x * c - y * s, at.y + x * s + y * c))
        .collect()
}

/// `p` inside or on the triangle `t`, either winding.
fn in_triangle(p: P2, t: &[P2; 3]) -> bool {
    let cross = |a: P2, b: P2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let (d0, d1, d2) = (cross(t[0], t[1]), cross(t[1], t[2]), cross(t[2], t[0]));
    let neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
    let pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
    !(neg && pos)
}

fn vtx(m: &Mesh, i: u32) -> Vec3 {
    let j = i as usize * 3;
    Vec3::new(m.positions[j] as f64, m.positions[j + 1] as f64, m.positions[j + 2] as f64)
}

fn corners(lo: Vec3, hi: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(lo.x, lo.y, lo.z),
        Vec3::new(hi.x, lo.y, lo.z),
        Vec3::new(lo.x, hi.y, lo.z),
        Vec3::new(hi.x, hi.y, lo.z),
        Vec3::new(lo.x, lo.y, hi.z),
        Vec3::new(hi.x, lo.y, hi.z),
        Vec3::new(lo.x, hi.y, hi.z),
        Vec3::new(hi.x, hi.y, hi.z),
    ]
}

fn on_plane(hs: &HalfSpace, p: Vec3, eps: f64) -> bool {
    ((p - hs.origin).dot(hs.normal)).abs() <= eps
}

/// `c` darkened a little as the surface turns away from the eye: a
/// surface square to it keeps its colour, one at 60 degrees is 89 % of it.
/// Quantised, so the triangles of one flat face share one shade and one
/// fill.
fn shade(c: Rgb, facing: f64) -> Rgb {
    let k = (0.78 + 0.22 * facing.clamp(0.0, 1.0)) * 64.0;
    let k = k.round() / 64.0;
    let f = |v: u8| (v as f64 * k).round().clamp(0.0, 255.0) as u8;
    Rgb(f(c.0), f(c.1), f(c.2))
}

fn darken(c: Rgb, k: f64) -> Rgb {
    let f = |v: u8| (v as f64 * k).round() as u8;
    Rgb(f(c.0), f(c.1), f(c.2))
}

fn mix_white(c: Rgb, t: f64) -> Rgb {
    let f = |v: u8| (v as f64 + (255.0 - v as f64) * t).round() as u8;
    Rgb(f(c.0), f(c.1), f(c.2))
}

/// The silhouette edges of the smooth parts of `mesh`: an edge two
/// triangles of the same face share, one facing the eye and one not. The
/// mesh's vertices are welded by position first, because a clipped mesh
/// gives every triangle its own.
fn silhouettes(mesh: &Mesh, basis: &ViewBasis, eps: f64) -> Vec<(Vec3, Vec3, f64)> {
    let q = eps.max(1e-12) * 10.0;
    let key = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut weld = |p: Vec3| {
        let n = ids.len() as u32;
        *ids.entry(key(p)).or_insert(n)
    };
    let mut edges: HashMap<(u32, u32), Vec<(usize, bool, Vec3, Vec3)>> = HashMap::new();
    for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
        let p = [vtx(mesh, tri[0]), vtx(mesh, tri[1]), vtx(mesh, tri[2])];
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.length() <= 1e-18 {
            continue;
        }
        let front = n.dot(basis.toward_eye) > 0.0;
        let v = [weld(p[0]), weld(p[1]), weld(p[2])];
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            let e = if a < b { (a, b) } else { (b, a) };
            edges.entry(e).or_default().push((t, front, p[k], p[(k + 1) % 3]));
        }
    }
    let mut out: Vec<(Vec3, Vec3, f64)> = edges
        .into_values()
        .filter(|ts| ts.len() == 2 && ts[0].1 != ts[1].1)
        .filter(|ts| mesh.face_ids.get(ts[0].0) == mesh.face_ids.get(ts[1].0))
        .map(|ts| {
            let (a, b) = (ts[0].2, ts[0].3);
            let depth = (basis.project(a).1 + basis.project(b).1) / 2.0;
            (a, b, depth)
        })
        .collect();
    // A hash map's order is not the same from run to run; the drawing must be.
    out.sort_by(|x, y| {
        x.2.total_cmp(&y.2)
            .then(x.0.x.total_cmp(&y.0.x))
            .then(x.0.y.total_cmp(&y.0.y))
            .then(x.0.z.total_cmp(&y.0.z))
            .then(x.1.x.total_cmp(&y.1.x))
            .then(x.1.y.total_cmp(&y.1.y))
    });
    out
}

/// A body of the colour look from a context's record: its tessellation,
/// colour, opacity and label.
pub fn view_body(ctx: &mut Context, id: cadpole_core::BodyId, layer: Option<String>) -> Result<ViewBody, String> {
    let (mesh, topo): (Mesh, Topology) = {
        let (m, t) = ctx.body_mesh(id).map_err(|e| e.to_string())?;
        (m.clone(), t.clone())
    };
    let b = ctx.body(id).map_err(|e| e.to_string())?;
    let (color, opacity, label) = paint_of(ctx, b);
    Ok(ViewBody { name: b.name.clone(), mesh, topo, layer, color, opacity, label })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Prim;
    use crate::views::{plan_view, section_view, Look};

    fn block(name: &str, x: f64, y: f64, z0: f64, w: f64, d: f64, h: f64, color: Rgb) -> ViewBody {
        let s = Sketch::new(Plane::XY.offset(z0)).with_loop(Loop::rect(P2::new(x + w / 2.0, y + d / 2.0), w, d));
        let solids = cadpole_core::kernel::extrude(&s, h).expect("extrude");
        let (mesh, topo) = cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess");
        ViewBody { color: Some(color), ..ViewBody::new(name, mesh, topo) }
    }

    fn color_opts() -> ViewOptions {
        ViewOptions { look: Look::Color, ..ViewOptions::default() }
    }

    fn fills(d: &Drawing, items: &[Item]) -> Vec<(Rgb, String, P2)> {
        let mut dd = d.clone();
        dd.items = items.to_vec();
        dd.flatten()
            .into_iter()
            .filter_map(|p| match p {
                Prim::Fill { style, outer, .. } => Some((style.color, style.layer, outer.segments[0].start())),
                _ => None,
            })
            .collect()
    }

    const RED: Rgb = Rgb(200, 40, 40);
    const BLUE: Rgb = Rgb(40, 40, 200);
    const SAND: Rgb = Rgb(220, 200, 160);

    #[test]
    fn a_plan_fills_the_floor_in_its_colour_and_the_cut_wall_on_top_in_its_own() {
        // A floor slab and a wall standing on it; the plan cuts the wall.
        let floor = block("floor", 0.0, 0.0, -10.0, 100.0, 60.0, 10.0, SAND);
        let wall = block("wall", 0.0, 0.0, 0.0, 100.0, 5.0, 30.0, RED);
        let mut d = Drawing::new();
        let g = plan_view(&mut d, &[floor, wall], 15.0, &color_opts());
        let f = fills(&d, &g.items);
        // The floor's top, seen, lightened; the wall's cut, exactly red, and
        // painted after everything seen.
        let seen = mix_white(SAND, PaintStyle::default().seen_lighten);
        let first_cut = f.iter().position(|(_, l, _)| l == layer::SECTION_HATCH).expect("a cut fill");
        assert!(f[..first_cut].iter().any(|(c, l, _)| *c == seen && l == FILL_LAYER), "{f:?}");
        assert!(f[first_cut..].iter().all(|(c, l, _)| *c == RED && l == layer::SECTION_HATCH), "{f:?}");
        let (lo, hi) = g.bounds.unwrap();
        assert!((hi.x - lo.x - 100.0).abs() < 1e-6 && (hi.y - lo.y - 60.0).abs() < 1e-6, "{lo:?} {hi:?}");
        // The cut is outlined in a darker red, not black.
        let mut dd = d.clone();
        dd.items = g.items.clone();
        let outline = dd
            .flatten()
            .into_iter()
            .find(|p| matches!(p, Prim::Path { style, closed: true, .. } if style.layer == layer::SECTION))
            .expect("a cut outline");
        assert_eq!(outline.style().color, darken(RED, 0.5));
    }

    /// The colour that ends up on top at `p`: the last fill, in paint
    /// order, one of whose triangles holds it.
    fn top_colour_at(d: &Drawing, items: &[Item], p: P2) -> Option<Rgb> {
        let mut dd = d.clone();
        dd.items = items.to_vec();
        let mut top = None;
        for prim in dd.flatten() {
            if let Prim::Fill { style, outer, holes, .. } = prim {
                for l in std::iter::once(&outer).chain(&holes) {
                    let t = [l.segments[0].start(), l.segments[1].start(), l.segments[2].start()];
                    if l.segments.len() == 3 && in_triangle(p, &t) {
                        top = Some(style.color);
                    }
                }
            }
        }
        top
    }

    /// A slab canted 5.7 degrees to the picture plane stands in front of a
    /// small block. The slab's triangles are long, and their centroids are
    /// farther from the eye than the block's front face, so a sort by
    /// centroid painted the block over the slab: the shards of a canted bay
    /// face with the wall behind it showing through. Where they overlap the
    /// slab is nearer, so it is painted last, and the block is hidden.
    #[test]
    fn a_canted_face_in_front_covers_what_is_behind_it() {
        // Seen from the south (-y), x right and z up; depth is -y.
        let s = Sketch::new(Plane::XY).with_loop(Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(100.0, 9.9),
            P2::new(100.0, 30.0),
            P2::new(0.0, 30.0),
        ]));
        let solids = cadpole_core::kernel::extrude(&s, 50.0).expect("extrude");
        let (mesh, topo) = cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess");
        let slab = ViewBody { color: Some(BLUE), ..ViewBody::new("slab", mesh, topo) };
        // Its front at y = 1: behind the slab, whose face is at y < 1 for
        // x < 10.
        let block = block("block", 0.0, 1.0, 10.0, 10.0, 4.0, 10.0, RED);
        let basis = crate::view::ViewBasis::looking_from(Vec3::new(0.0, -1.0, 0.0), Vec3::Z);
        let mut d = Drawing::new();
        let g = crate::views::elevation_view(&mut d, &[block, slab], &basis, &color_opts());
        for (x, z) in [(5.0, 15.0), (2.0, 11.0), (8.0, 19.0), (9.5, 12.0)] {
            let top = top_colour_at(&d, &g.items, P2::new(x, z)).expect("something is painted there");
            assert_ne!(top.0, top.2, "at ({x}, {z}) the top fill is {top:?}");
            assert!(top.2 > top.0, "the slab (blue) is on top at ({x}, {z}), not the block behind it: {top:?}");
        }
    }

    #[test]
    fn nearer_surfaces_are_painted_after_farther_ones() {
        // A low blue block and a taller red one beside it, both under the
        // cut: the red top (z = 20) is nearer the eye than the blue (z = 5).
        let low = block("low", 0.0, 0.0, 0.0, 40.0, 40.0, 5.0, BLUE);
        let tall = block("tall", 20.0, 20.0, 0.0, 40.0, 40.0, 20.0, RED);
        let mut d = Drawing::new();
        let g = plan_view(&mut d, &[tall, low], 50.0, &color_opts());
        let f = fills(&d, &g.items);
        let k = PaintStyle::default().seen_lighten;
        let blue_at = f.iter().position(|(c, ..)| *c == mix_white(BLUE, k)).expect("blue top");
        let red_at = f.iter().position(|(c, ..)| *c == mix_white(RED, k)).expect("red top");
        assert!(blue_at < red_at, "the farther top first: {f:?}");
        assert!(!f.iter().any(|(_, l, _)| l == layer::SECTION_HATCH), "nothing is cut");
    }

    #[test]
    fn a_section_looks_at_what_is_behind_the_cut() {
        // Two blocks along x; the section at x = 30 looking toward -x cuts
        // the first and sees the second's face beyond it.
        let a = block("a", 0.0, 0.0, 0.0, 40.0, 20.0, 10.0, RED);
        let b = block("b", -60.0, -10.0, 0.0, 20.0, 40.0, 25.0, BLUE);
        let plane = Plane::new(cadpole_core::geom::Vec3::new(30.0, 0.0, 0.0), cadpole_core::geom::Vec3::X, cadpole_core::geom::Vec3::Y).unwrap();
        let mut d = Drawing::new();
        let g = section_view(&mut d, &[a, b], &plane, None, true, &color_opts());
        let f = fills(&d, &g.items);
        assert!(f.iter().any(|(c, l, _)| *c == RED && l == layer::SECTION_HATCH), "a is cut");
        assert!(f.iter().any(|(c, l, _)| *c == mix_white(BLUE, PaintStyle::default().seen_lighten) && l == FILL_LAYER), "b is seen: {f:?}");
        // The frame: x along y, y up; b is 25 tall.
        let (_, hi) = g.bounds.unwrap();
        assert!((hi.y - 25.0).abs() < 1e-6, "{hi:?}");
    }

    #[test]
    fn a_glyph_body_is_written_once_as_text_at_its_model_size() {
        let floor = block("floor", 0.0, 0.0, -10.0, 100.0, 60.0, 10.0, SAND);
        let label = BodyLabel {
            group: "room_1".into(),
            text: "Kitchen".into(),
            cap_height: 6.0,
            anchor: Anchor::CENTER,
            bold: false,
            plane: Plane::XY.offset(1.0),
            at: P2::new(50.0, 30.0),
        };
        // Two glyph bodies of the same label (two separate letters).
        let mut g1 = block("K", 40.0, 28.0, 1.0, 4.0, 4.0, 0.5, Rgb(20, 30, 40));
        g1.label = Some(label.clone());
        let mut g2 = block("n", 56.0, 28.0, 1.0, 4.0, 4.0, 0.5, Rgb(20, 30, 40));
        g2.label = Some(label);
        let mut d = Drawing::new();
        let g = plan_view(&mut d, &[floor, g1, g2], 15.0, &color_opts());
        let texts: Vec<&TextItem> = g.items.iter().filter_map(|i| if let Item::Text(t) = i { Some(t) } else { None }).collect();
        assert_eq!(texts.len(), 1, "one text per label");
        let t = texts[0];
        assert_eq!(t.text, "Kitchen");
        assert!(t.model_height && (t.height_mm - 6.0).abs() < 1e-12);
        assert!((t.at.x - 50.0).abs() < 1e-9 && (t.at.y - 30.0).abs() < 1e-9, "{:?}", t.at);
        assert_eq!(t.font, FontRef::Builtin);
        assert_eq!(t.style.color, Some(Rgb(20, 30, 40)));
        // Painted after the floor it lies on, and the glyphs' own surfaces
        // are not drawn.
        let text_at = g.items.iter().position(|i| matches!(i, Item::Text(_))).unwrap();
        let f = fills(&d, &g.items[..text_at]);
        assert!(!f.is_empty());
        assert!(!fills(&d, &g.items).iter().any(|(c, ..)| *c == mix_white(Rgb(20, 30, 40), PaintStyle::default().seen_lighten)));
        // Seen edge-on, from the side, the label is left out.
        let plane = Plane::new(cadpole_core::geom::Vec3::new(0.0, 30.0, 0.0), cadpole_core::geom::Vec3::Y, cadpole_core::geom::Vec3::new(-1.0, 0.0, 0.0)).unwrap();
        let mut d2 = Drawing::new();
        let floor = block("floor", 0.0, 0.0, -10.0, 100.0, 60.0, 10.0, SAND);
        let mut g3 = block("K", 40.0, 28.0, 1.0, 4.0, 4.0, 0.5, Rgb(20, 30, 40));
        g3.label = Some(BodyLabel { group: "x".into(), text: "X".into(), cap_height: 6.0, anchor: Anchor::CENTER, bold: false, plane: Plane::XY.offset(1.0), at: P2::new(42.0, 30.0) });
        let side = section_view(&mut d2, &[floor, g3], &plane, None, true, &color_opts());
        assert!(!side.items.iter().any(|i| matches!(i, Item::Text(_))));
    }

    /// A room name on a lower level is under the floor above it: a render
    /// hides it, so the drawing leaves it out -- a reader searching the PDF
    /// finds only the words it can see. Half covered, it is drawn.
    #[test]
    fn a_label_wholly_under_something_nearer_is_left_out() {
        let label = |group: &str, x: f64| BodyLabel {
            group: group.into(),
            text: "Den".into(),
            cap_height: 4.0,
            anchor: Anchor::CENTER,
            bold: false,
            plane: Plane::XY.offset(1.0),
            at: P2::new(x, 30.0),
        };
        let glyph = |group: &str, x: f64| {
            let mut g = block("g", x - 3.0, 28.0, 1.0, 6.0, 4.0, 0.5, Rgb(20, 30, 40));
            g.label = Some(label(group, x));
            g
        };
        let below = block("lower floor", 0.0, 0.0, -10.0, 100.0, 60.0, 10.0, SAND);
        // An upper floor over the left half only, below the cut.
        let upper = block("upper floor", 0.0, 0.0, 8.0, 50.0, 60.0, 2.0, BLUE);
        let mut d = Drawing::new();
        let g = plan_view(&mut d, &[below, upper, glyph("under", 20.0), glyph("open", 75.0), glyph("edge", 50.0)], 15.0, &color_opts());
        let texts: Vec<P2> = g.items.iter().filter_map(|i| if let Item::Text(t) = i { Some(t.at) } else { None }).collect();
        assert_eq!(texts.len(), 2, "the open label and the half-covered one: {texts:?}");
        assert!(texts.iter().all(|p| p.x > 40.0), "{texts:?}");
    }

    #[test]
    fn the_same_bodies_paint_the_same_items() {
        let a = || [block("a", 0.0, 0.0, 0.0, 40.0, 20.0, 10.0, RED), block("b", 10.0, 30.0, 0.0, 20.0, 20.0, 30.0, BLUE)];
        let (mut d1, mut d2) = (Drawing::new(), Drawing::new());
        let g1 = plan_view(&mut d1, &a(), 20.0, &color_opts());
        let g2 = plan_view(&mut d2, &a(), 20.0, &color_opts());
        assert_eq!(g1.items, g2.items);
    }

    #[test]
    fn colours_come_back_as_the_hex_a_script_wrote() {
        // `material(color="#c9774f")` is stored linear; the drawing shows
        // the sRGB the script wrote, to the unit.
        let to_lin = |c: u8| {
            let s = c as f64 / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        for (r, g, b) in [(0xc9, 0x77, 0x4f), (0, 0, 0), (255, 255, 255), (0x1d, 0x27, 0x33)] {
            assert_eq!(srgb([to_lin(r), to_lin(g), to_lin(b)]), Rgb(r, g, b));
        }
    }
}
