//! The line overlay: ground grid, feature edges, silhouettes and
//! selection highlights, drawn as ink into `Frame::display` (display-linear)
//! after the tonemap.
//!
//! Anti-aliasing. Ink coverage is computed analytically: a segment is a
//! round-capped capsule and a pixel's coverage is `clamp(halfwidth + 0.5 -
//! distance, 0, 1)`, which is exact for an axis-aligned edge and within a
//! few percent everywhere else. The coverage is evaluated in *unjittered*
//! screen space, identical in every anti-aliasing pass, while the depth test
//! reads that pass's jittered G-buffer. Averaging N passes of `mix(display,
//! ink, c)` with the same `c` is exactly `mix(average_display, ink, c)`, so
//! lines keep their nominal width and darkness at every sample count (drawing
//! jittered ink instead would convolve the line with the sample pattern and
//! blur it by up to half a pixel, and dividing coverage by the sample count
//! would make lines fade as quality goes up). Only the depth test and the
//! screen-space silhouette detector vary per pass, which is what antialiases
//! them.
//!
//! Widths are in pixels and so are resolution independent. Overlapping
//! segments of one layer combine with `max`, not by blending twice, so a
//! polyline does not darken at its joins; separate layers (grid, silhouette,
//! crease, highlight) do blend in order.
//!
//! Depth. A line lying exactly on the surface it belongs to must neither
//! z-fight nor float, so the test is a soft one with a slope-scaled bias:
//! ink fades out once it is behind the depth buffer by more than the local
//! per-pixel depth gradient. Ink behind translucent geometry is attenuated by
//! [`oit::transmittance_at`] instead of being hidden.

use super::aa::par_bands2;
use super::{
    Ctx, DebugView, EdgeMode, Frame, GridMode, HIGHLIGHT, NO_ITEM, View, add, debug, dot, from_srgb8, lerp3, mul, oit,
    scale, smoothstep, sub,
};
use crate::geom::Vec3;

/// Rows per parallel band.
const BAND: usize = 32;

/// Feature-edge ink (sRGB8 -> display-linear), kept soft: the edge reads as a
/// dark grey line at a fraction of full opacity rather than a black hairline.
const EDGE_INK: [u8; 3] = [22, 24, 28];
const GRID_MINOR_INK: [u8; 3] = [48, 50, 56];
const GRID_MAJOR_INK: [u8; 3] = [76, 79, 87];
const GRID_AXIS_X_INK: [u8; 3] = [128, 58, 58];
const GRID_AXIS_Y_INK: [u8; 3] = [64, 118, 66];

/// Per-layer opacity. Silhouettes are a little heavier than interior creases,
/// which is what makes a CAD viewport read as solid.
const ALPHA_SILHOUETTE: f32 = 0.62;
const ALPHA_CREASE: f32 = 0.42;
/// `EdgeMode::All` keeps the older, harder look.
const ALPHA_ALL: f32 = 0.62;
const ALPHA_GRID_MINOR: f32 = 0.45;
const ALPHA_GRID_MAJOR: f32 = 0.70;
const ALPHA_GRID_AXIS: f32 = 0.80;

const GRID_WIDTH_PX: f32 = 1.0;
const HIGHLIGHT_WIDTH_PX: f32 = 2.8;
/// The `wire` debug view never draws wider than this, however wide feature
/// edges are set.
const WIRE_HALF_WIDTH_PX: f32 = 0.5;
const ALPHA_WIRE: f32 = 0.8;

/// World-space length of one screen-space piece of a projected polyline.
const PIECE_PX: f32 = 6.0;

fn ink(c: [u8; 3]) -> [f32; 3] {
    c.map(from_srgb8)
}

/// A projected segment: screen (x, y, view z) endpoints, half width and
/// opacity.
#[derive(Clone, Copy)]
struct Seg {
    a: [f32; 3],
    b: [f32; 3],
    hw: f32,
    alpha: f32,
}

/// One overlay layer's per-pixel ink coverage and the depth of the element
/// that produced it.
struct Ink {
    cov: Vec<f32>,
    z: Vec<f32>,
    any: bool,
}

impl Ink {
    fn new(n: usize) -> Ink {
        Ink { cov: vec![0.0; n], z: vec![f32::INFINITY; n], any: false }
    }

    fn clear(&mut self) {
        if self.any {
            self.cov.fill(0.0);
            self.z.fill(f32::INFINITY);
            self.any = false;
        }
    }
}

/// Grid (per `opts.grid_mode()`), then silhouettes, feature edges and
/// highlight edges (per `opts.edge_mode()`), composited in that order.
pub(crate) fn overlay(frame: &mut Frame, ctx: &Ctx<'_>, oit: &oit::Buffers) {
    let n = (frame.w as usize) * (frame.h as usize);
    match ctx.opts.grid_mode() {
        GridMode::Off => {}
        GridMode::Infinite => grid_infinite(frame, ctx, oit),
        GridMode::Finite => {
            let mut layer = Ink::new(n);
            grid_finite(frame, ctx, &mut layer, oit);
        }
    }
    let mode = ctx.opts.edge_mode();
    let highlighted = ctx.items.iter().any(|it| !it.highlight_edges.is_empty());
    if mode == EdgeMode::Off && !highlighted {
        return;
    }
    if mode != EdgeMode::Off {
        // The `seams` and `wire` debug views are this overlay with a
        // different question: they replace the feature-edge layer, keep the
        // silhouettes, and answer to `draw_edges` like any other ink, so
        // `--no-edges` still shows the bare grey body underneath.
        match ctx.opts.debug {
            Some(DebugView::Seams) => seams_overlay(frame, ctx, oit),
            Some(DebugView::Wire) => wire_overlay(frame, ctx, oit),
            _ => feature_overlay(frame, ctx, oit, mode),
        }
    }
    if highlighted {
        let mut layer = Ink::new(n);
        let hw = HIGHLIGHT_WIDTH_PX * 0.5;
        let view = unjittered(ctx.view);
        let mut segs = Vec::new();
        for item in ctx.items {
            for poly in item.highlight_edges {
                push_polyline(&mut segs, &view, poly, hw, 1.0, None);
            }
        }
        // Selection ink is never attenuated: it has to be findable even
        // through glass.
        splat(&mut layer, frame.w as usize, frame.h as usize, &segs, ctx.threads);
        blend(frame, ctx, &layer, HIGHLIGHT, None);
    }
}

/// The view of the current pass with the jitter removed, so overlay geometry
/// lands in the same place in every pass (see the module comment).
fn unjittered(view: &View) -> View {
    view.with_jitter(0.5, 0.5)
}

// ---------------------------------------------------------------------------
// Feature edges and the crease filter.

/// The normal render's ink: creases (or every feature edge) and the
/// silhouettes over them, in the edge colour.
fn feature_overlay(frame: &mut Frame, ctx: &Ctx<'_>, oit: &oit::Buffers, mode: EdgeMode) {
    let n = (frame.w as usize) * (frame.h as usize);
    let mut layer = Ink::new(n);
    if matches!(mode, EdgeMode::Creases | EdgeMode::All) {
        feature_edges(frame, ctx, &mut layer, mode);
    }
    if ctx.opts.draw_silhouettes {
        // Outlines run after the creases so they can stand back where a
        // feature edge already draws the same line: a screen-space outline is
        // a whole-pixel mask, and laying it on top of an analytic line turns
        // the line into a dotted one at every staircase step.
        let mut outline = Ink::new(n);
        silhouettes(frame, ctx, &mut outline, oit, &layer);
        blend(frame, ctx, &outline, ink(EDGE_INK), Some(oit));
    }
    blend(frame, ctx, &layer, ink(EDGE_INK), Some(oit));
}

/// Rasterizes the feature edges of every item, filtered by the crease rule,
/// into `layer` (the caller composites it).
fn feature_edges(frame: &mut Frame, ctx: &Ctx<'_>, layer: &mut Ink, mode: EdgeMode) {
    let view = unjittered(ctx.view);
    let hw = ctx.opts.edge_width_px.max(0.2) * 0.5;
    let thr = ctx.opts.edge_crease_degrees.clamp(0.0, 180.0);
    let est = Estimator {
        normal: &frame.normal,
        depth: &frame.depth,
        w: frame.w as usize,
        h: frame.h as usize,
        cos_thr: cos_degrees(thr),
        jump: 0.01 * ctx.scene.diag,
    };
    let mut segs: Vec<Seg> = Vec::new();
    for item in ctx.items {
        for (i, poly) in item.edges.iter().enumerate() {
            if mode == EdgeMode::All {
                push_polyline(&mut segs, &view, poly, hw, 1.0, None);
                continue;
            }
            match item.edge_dihedrals.get(i) {
                // Real crease data (tessellate::EdgeInfo::dihedral_degrees):
                // 0 is a tangent seam between two patches of one smooth
                // surface, -1 an open boundary (always a real edge).
                Some(&deg) => {
                    let w = if deg < 0.0 { 1.0 } else { smoothstep(thr - 4.0, thr + 4.0, deg as f32) };
                    if w > 0.0 {
                        push_polyline(&mut segs, &view, poly, hw, w, None);
                    }
                }
                // No topology: estimate the dihedral from the G-buffer
                // normals on either side of the line.
                None => push_polyline(&mut segs, &view, poly, hw, 1.0, Some(&est)),
            }
        }
    }
    let alpha = if mode == EdgeMode::All { ALPHA_ALL } else { ALPHA_CREASE };
    for s in &mut segs {
        s.alpha *= alpha;
    }
    splat(layer, frame.w as usize, frame.h as usize, &segs, ctx.threads);
}

#[inline]
fn cos_degrees(deg: f32) -> f32 {
    (deg * std::f32::consts::PI / 180.0).cos()
}

/// Fallback crease detector for callers with no topology: samples the
/// G-buffer normals a couple of pixels either side of the line and keeps the
/// line only where the two sides disagree by more than the crease angle, or
/// where one side is a different surface altogether.
struct Estimator<'a> {
    normal: &'a [[f32; 3]],
    depth: &'a [f32],
    w: usize,
    h: usize,
    cos_thr: f32,
    jump: f32,
}

impl Estimator<'_> {
    /// Distance either side of the line, in pixels.
    const OFFSET: f32 = 2.0;
    /// Half width of the smooth band around the threshold, in cosine units.
    const BAND: f32 = 0.05;

    fn at(&self, x: f32, y: f32) -> Option<([f32; 3], f32)> {
        if !(x >= 0.0 && y >= 0.0) {
            return None;
        }
        let (xi, yi) = (x as usize, y as usize);
        if xi >= self.w || yi >= self.h {
            return None;
        }
        let i = yi * self.w + xi;
        Some((self.normal[i], self.depth[i]))
    }

    fn weight(&self, mid: [f32; 3], dir: [f32; 2]) -> f32 {
        let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
        if !(len > 1e-6) {
            return 1.0;
        }
        let (px, py) = (-dir[1] / len * Self::OFFSET, dir[0] / len * Self::OFFSET);
        let (Some((_, d0)), Some((na, da)), Some((nb, db))) = (
            self.at(mid[0], mid[1]),
            self.at(mid[0] + px, mid[1] + py),
            self.at(mid[0] - px, mid[1] - py),
        ) else {
            return 1.0;
        };
        if !d0.is_finite() || !da.is_finite() || !db.is_finite() {
            // One side is background: a silhouette, always drawn.
            return 1.0;
        }
        // Curvature, not slope: a floor seen at a grazing angle changes depth
        // fast between the two samples without being an edge at all, while a
        // step between two surfaces breaks the symmetry.
        if (da + db - 2.0 * d0).abs() > self.jump {
            return 1.0;
        }
        smoothstep(self.cos_thr + Self::BAND, self.cos_thr - Self::BAND, dot(na, nb))
    }
}

/// Projects a polyline and appends its pieces; see [`push_segment`].
fn push_polyline(segs: &mut Vec<Seg>, view: &View, poly: &[Vec3], hw: f32, alpha: f32, est: Option<&Estimator<'_>>) {
    for seg in poly.windows(2) {
        let a = [seg[0].x as f32, seg[0].y as f32, seg[0].z as f32];
        let b = [seg[1].x as f32, seg[1].y as f32, seg[1].z as f32];
        push_segment(segs, view, a, b, hw, alpha, est);
    }
}

/// Projects one world-space segment and appends its pieces. Segments are
/// subdivided in world space so the screen-linear depth of each piece stays
/// close to the perspective-correct depth in the G-buffer (a long edge
/// interpolated affinely would sink below the surface it lies on).
fn push_segment(
    segs: &mut Vec<Seg>,
    view: &View,
    a: [f32; 3],
    b: [f32; 3],
    hw: f32,
    alpha: f32,
    est: Option<&Estimator<'_>>,
) {
    let (Some(sa), Some(sb)) = (view.project(a), view.project(b)) else { return };
    let len = ((sb[0] - sa[0]).powi(2) + (sb[1] - sa[1]).powi(2)).sqrt();
    if !len.is_finite() {
        return;
    }
    let n = if view.is_orthographic() { 1 } else { (len / PIECE_PX).ceil().clamp(1.0, 4096.0) as u32 };
    let mut prev = sa;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        let Some(sp) = view.project(p) else { break };
        let mut alpha = alpha;
        if let Some(est) = est {
            let mid = [(prev[0] + sp[0]) * 0.5, (prev[1] + sp[1]) * 0.5, 0.0];
            alpha *= est.weight(mid, [sp[0] - prev[0], sp[1] - prev[1]]);
        }
        if alpha > 0.0 {
            segs.push(Seg { a: prev, b: sp, hw, alpha });
        }
        prev = sp;
    }
}

// ---------------------------------------------------------------------------
// The `seams` and `wire` debug views.

/// Every feature edge, classified by the crease rule instead of filtered by
/// it: at or above `edge_crease_degrees` (and every open boundary) in
/// black, below it — the seams the rule leaves undrawn and the shading
/// smooths over — in blue. The classification is hard where the normal
/// overlay's is a soft band, because the question this answers is which side
/// of the line an edge fell on. Items without topology go through the
/// G-buffer estimator, as always, and land in the crease layer.
fn seams_overlay(frame: &mut Frame, ctx: &Ctx<'_>, oitb: &oit::Buffers) {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let view = unjittered(ctx.view);
    let hw = ctx.opts.edge_width_px.max(0.2) * 0.5;
    let thr = ctx.opts.edge_crease_degrees.clamp(0.0, 180.0);
    let est = Estimator {
        normal: &frame.normal,
        depth: &frame.depth,
        w,
        h,
        cos_thr: cos_degrees(thr),
        jump: 0.01 * ctx.scene.diag,
    };
    let (mut creases, mut seams) = (Vec::new(), Vec::new());
    for item in ctx.items {
        for (i, poly) in item.edges.iter().enumerate() {
            match item.edge_dihedrals.get(i) {
                Some(&deg) if deg < 0.0 || deg as f32 >= thr => push_polyline(&mut creases, &view, poly, hw, 1.0, None),
                Some(_) => push_polyline(&mut seams, &view, poly, hw, 1.0, None),
                None => push_polyline(&mut creases, &view, poly, hw, 1.0, Some(&est)),
            }
        }
    }
    // Seams first and creases over them, so where the two meet at a vertex
    // the crease wins.
    let mut layer = Ink::new(w * h);
    splat(&mut layer, w, h, &seams, ctx.threads);
    blend(frame, ctx, &layer, debug::SEAM_INK, Some(oitb));
    layer.clear();
    splat(&mut layer, w, h, &creases, ctx.threads);
    if ctx.opts.draw_silhouettes {
        let mut outline = Ink::new(w * h);
        silhouettes(frame, ctx, &mut outline, oitb, &layer);
        blend(frame, ctx, &outline, debug::CREASE_INK, Some(oitb));
    }
    blend(frame, ctx, &layer, debug::CREASE_INK, Some(oitb));
}

/// `wire`: every triangle edge of every mesh, once, depth-tested against the
/// body it belongs to. Thin, because a wireframe at the feature-edge width
/// over a dense band is a solid block of ink, which is the opposite of what
/// the view is for. Edges are deduplicated by vertex index, so an edge two
/// faces share in an unwelded mesh is drawn twice, and the layer's `max`
/// rule keeps that from darkening it.
fn wire_overlay(frame: &mut Frame, ctx: &Ctx<'_>, oitb: &oit::Buffers) {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let view = unjittered(ctx.view);
    let hw = (ctx.opts.edge_width_px.max(0.2) * 0.5).min(WIRE_HALF_WIDTH_PX);
    let mut segs = Vec::new();
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for item in ctx.items {
        let m = item.mesh;
        seen.clear();
        seen.reserve(m.indices.len());
        let at = |i: u32| {
            let i = i as usize * 3;
            [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
        };
        for tri in m.indices.chunks_exact(3) {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                if seen.insert(if a < b { (a, b) } else { (b, a) }) {
                    push_segment(&mut segs, &view, at(a), at(b), hw, ALPHA_WIRE, None);
                }
            }
        }
    }
    let mut layer = Ink::new(w * h);
    splat(&mut layer, w, h, &segs, ctx.threads);
    if ctx.opts.draw_silhouettes {
        let mut outline = Ink::new(w * h);
        silhouettes(frame, ctx, &mut outline, oitb, &layer);
        blend(frame, ctx, &outline, debug::WIRE_INK, Some(oitb));
    }
    blend(frame, ctx, &layer, debug::WIRE_INK, Some(oitb));
}

// ---------------------------------------------------------------------------
// Silhouettes.

/// Screen-space outline: a pixel is inked when one of its four neighbours is
/// the background, a different body, or a surface that jumps away from it,
/// and when it is the nearer of the two, so outlines hug the object and never
/// bleed onto the background. Translucent bodies are outlined from the OIT
/// near-depth field for the same reason.
///
/// The depth test is a curvature test, not a plain difference: a floor seen
/// at a grazing angle can move by several millimetres from one pixel to the
/// next without being an edge, so a jump only counts when it is several times
/// the depth slope already present on the opposite side of the pixel.
///
/// `creases` is the feature-edge layer drawn just before; pixels it already
/// inked (or the pixels next to them) are left alone.
fn silhouettes(frame: &Frame, ctx: &Ctx<'_>, layer: &mut Ink, oitb: &oit::Buffers, creases: &Ink) {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let floor = 0.02 * ctx.scene.diag;
    let (depth, item, near) = (&frame.depth, &frame.item, &oitb.near_z);
    let glass = !oitb.is_empty();
    let ink_near = &creases.cov;
    par_bands2(&mut layer.cov, &mut layer.z, w, BAND, ctx.threads, |y0, cov, zb| {
        let rows = cov.len() / w;
        let at = |j: usize| if glass { depth[j].min(near[j]) } else { depth[j] };
        for r in 0..rows {
            let y = y0 + r;
            for x in 0..w {
                let i = y * w + x;
                let d = at(i);
                if !d.is_finite() {
                    continue;
                }
                // A feature edge already draws this line.
                let mut drawn = ink_near[i] > 0.15;
                if x > 0 {
                    drawn |= ink_near[i - 1] > 0.15;
                }
                if x + 1 < w {
                    drawn |= ink_near[i + 1] > 0.15;
                }
                if y > 0 {
                    drawn |= ink_near[i - w] > 0.15;
                }
                if y + 1 < h {
                    drawn |= ink_near[i + w] > 0.15;
                }
                if drawn {
                    continue;
                }
                let mut c = 0.0f32;
                for (step, lo, hi) in [(1usize, x > 0, x + 1 < w), (w, y > 0, y + 1 < h)] {
                    for (fwd, ok) in [(true, hi), (false, lo)] {
                        if !ok {
                            continue;
                        }
                        let j = if fwd { i + step } else { i - step };
                        let dn = at(j);
                        let v = if !dn.is_finite() {
                            1.0
                        } else if dn < d {
                            // The neighbour is nearer; it owns the outline.
                            0.0
                        } else if item[i] != item[j] && item[i] != NO_ITEM && item[j] != NO_ITEM {
                            1.0
                        } else {
                            // Slope on the opposite side, as the expectation
                            // for a continuous surface.
                            let back = if fwd { i.checked_sub(step) } else { i.checked_add(step) };
                            let slope = match back {
                                Some(b) if b < depth.len() && (if fwd { lo } else { hi }) => {
                                    let db = at(b);
                                    if db.is_finite() { (d - db).abs() } else { 0.0 }
                                }
                                _ => 0.0,
                            };
                            let thr = floor.max(4.0 * slope);
                            smoothstep(thr, thr * 2.0, dn - d)
                        };
                        if v > c {
                            c = v;
                        }
                    }
                }
                if c > 0.0 {
                    cov[r * w + x] = c * ALPHA_SILHOUETTE;
                    zb[r * w + x] = d;
                }
            }
        }
    });
    layer.any = true;
}

// ---------------------------------------------------------------------------
// Grid.

/// Unbounded ground grid, rasterized per pixel: for every pixel whose ray
/// hits the ground plane, the distance to the nearest grid line is measured
/// in world units and divided by the world size of a pixel there, so coverage
/// is analytic at any angle. Minor lines fade out as their screen spacing
/// approaches a few pixels (no moire in the distance), everything fades with
/// distance from the model, and in perspective the plane dissolves into the
/// background well before the horizon.
fn grid_infinite(frame: &mut Frame, ctx: &Ctx<'_>, oitb: &oit::Buffers) {
    let view = unjittered(ctx.view);
    let sc = ctx.scene;
    let step = nice_step((sc.diag as f64 / 8.0).max(1e-9)) as f32;
    let major = step * 10.0;
    let plane = sc.ground_z;
    let (w, h) = (frame.w as usize, frame.h as usize);
    let (r0, r1) = (0.8 * sc.diag, 2.8 * sc.diag);
    let hw = GRID_WIDTH_PX * 0.5;
    let diag = sc.diag;
    // Everything on the plane is measured from `anchor`, a major grid
    // crossing near the eye, so it stays small: a world point near 1e7 mm
    // is only known to ~1 mm in f32, coarser than a pixel, and measuring
    // the line distances and the pixel's footprint in world coordinates
    // drew the grid as bands. The anchor is a multiple of the major
    // spacing, so the lines fall where they always did.
    let anchor = [
        (f64::from(view.eye[0]) / f64::from(major)).round() * f64::from(major),
        (f64::from(view.eye[1]) / f64::from(major)).round() * f64::from(major),
    ];
    let eye_local = [(f64::from(view.eye[0]) - anchor[0]) as f32, (f64::from(view.eye[1]) - anchor[1]) as f32];
    let plane_below_eye = (f64::from(plane) - f64::from(view.eye[2])) as f32;
    let center = [(f64::from(sc.center[0]) - anchor[0]) as f32, (f64::from(sc.center[1]) - anchor[1]) as f32];
    // (point on the plane relative to `anchor`, view depth) for a pixel.
    let hit = |sx: f32, sy: f32| -> Option<([f32; 2], f32)> {
        let (o, d) = view.eye_ray(sx, sy);
        if d[2].abs() < 1e-12 {
            return None;
        }
        let t = (plane_below_eye - o[2]) / d[2];
        if !(t > 1e-4) || !t.is_finite() {
            return None;
        }
        Some(([eye_local[0] + o[0] + d[0] * t, eye_local[1] + o[1] + d[1] * t], t))
    };
    let (minor_ink, major_ink) = (ink(GRID_MINOR_INK), ink(GRID_MAJOR_INK));
    let (ax_ink, ay_ink) = (ink(GRID_AXIS_X_INK), ink(GRID_AXIS_Y_INK));
    let glass = !oitb.is_empty();
    let depth = &frame.depth;
    par_bands2(&mut frame.display, &mut frame.alpha, w, BAND, ctx.threads, |y0, band, alphas| {
        for (k, (px, pa)) in band.iter_mut().zip(alphas.iter_mut()).enumerate() {
            let i = y0 * w + k;
            let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            let Some((p, z)) = hit(x, y) else { continue };
            let dist = ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2)).sqrt();
            let fade = 1.0 - smoothstep(r0, r1, dist);
            if fade <= 0.0 {
                continue;
            }
            let vis = visibility(depth, w, h, i, z, diag);
            if vis <= 0.0 {
                continue;
            }
            // World size of one pixel on the plane, per world axis.
            let (Some((qx, _)), Some((qy, _))) = (hit(x + 1.0, y), hit(x, y + 1.0)) else {
                continue;
            };
            let wx = (qx[0] - p[0]).abs().max((qy[0] - p[0]).abs()).max(1e-9);
            let wy = (qx[1] - p[1]).abs().max((qy[1] - p[1]).abs()).max(1e-9);
            let line = |coord: f32, spacing: f32, per_px: f32| -> f32 {
                let q = coord / spacing;
                let g = (q - q.round()).abs() * spacing / per_px;
                (hw + 0.5 - g).clamp(0.0, 1.0)
            };
            // Spacing in pixels; below a few pixels the lines would merge
            // into moire, so they are faded out instead.
            let fx = smoothstep(4.0, 14.0, step / wx);
            let fy = smoothstep(4.0, 14.0, step / wy);
            let mx = smoothstep(4.0, 14.0, major / wx);
            let my = smoothstep(4.0, 14.0, major / wy);
            let c_minor = (line(p[0], step, wx) * fx).max(line(p[1], step, wy) * fy);
            let c_major = (line(p[0], major, wx) * mx).max(line(p[1], major, wy) * my);
            // The axes are at world 0: the local point plus the anchor, in f64.
            let (world_x, world_y) = ((p[0] as f64 + anchor[0]) as f32, (p[1] as f64 + anchor[1]) as f32);
            let c_ax = ((hw + 0.5) - world_y.abs() / wy).clamp(0.0, 1.0) * my;
            let c_ay = ((hw + 0.5) - world_x.abs() / wx).clamp(0.0, 1.0) * mx;
            let amt = fade * vis;
            let tr = if glass { oit::transmittance_at(oitb, i, z) } else { [1.0; 3] };
            let mut col = *px;
            let mut clear = 1.0 - *pa;
            for (ink, a) in [
                (minor_ink, c_minor * ALPHA_GRID_MINOR * amt),
                (major_ink, c_major * ALPHA_GRID_MAJOR * amt),
                (ax_ink, c_ax * ALPHA_GRID_AXIS * amt),
                (ay_ink, c_ay * ALPHA_GRID_AXIS * amt),
            ] {
                col = lerp3(col, mul(ink, tr), a);
                clear *= 1.0 - a;
            }
            *px = col;
            *pa = 1.0 - clear;
        }
    });
}

/// The square patch of grid lines for `--grid finite`.
fn grid_finite(frame: &mut Frame, ctx: &Ctx<'_>, layer: &mut Ink, oitb: &oit::Buffers) {
    if ctx.items.iter().all(|it| it.mesh.positions.is_empty()) {
        return;
    }
    let sc = ctx.scene;
    let size = (sc.diag as f64).max(1.0);
    let step = nice_step(size / 8.0);
    let z = sc.ground_z;
    let ext = (size * 1.2 / step).ceil() as i32;
    let cx = ((sc.lo[0] as f64 + sc.hi[0] as f64) / 2.0 / step).round() * step;
    let cy = ((sc.lo[1] as f64 + sc.hi[1] as f64) / 2.0 / step).round() * step;
    let view = unjittered(ctx.view);
    let span = ext as f64 * step;
    let hw = GRID_WIDTH_PX * 0.5;
    let (mut minor, mut axis) = (Vec::new(), Vec::new());
    for i in -ext..=ext {
        let o = i as f64 * step;
        let target = if i == 0 { &mut axis } else { &mut minor };
        for (a, b) in [
            ([(cx + o) as f32, (cy - span) as f32, z], [(cx + o) as f32, (cy + span) as f32, z]),
            ([(cx - span) as f32, (cy + o) as f32, z], [(cx + span) as f32, (cy + o) as f32, z]),
        ] {
            let poly = [Vec3::new(a[0] as f64, a[1] as f64, a[2] as f64), Vec3::new(b[0] as f64, b[1] as f64, b[2] as f64)];
            push_polyline(target, &view, &poly, hw, 1.0, None);
        }
    }
    let (w, h) = (frame.w as usize, frame.h as usize);
    for s in &mut minor {
        s.alpha *= ALPHA_GRID_MINOR;
    }
    splat(layer, w, h, &minor, ctx.threads);
    blend(frame, ctx, layer, ink(GRID_MINOR_INK), Some(oitb));
    layer.clear();
    for s in &mut axis {
        s.alpha *= ALPHA_GRID_MAJOR;
    }
    splat(layer, w, h, &axis, ctx.threads);
    blend(frame, ctx, layer, ink(GRID_MAJOR_INK), Some(oitb));
    layer.clear();
}

// ---------------------------------------------------------------------------
// Rasterization and compositing.

/// Bins segments into row bands and splats them in parallel. Bands own
/// disjoint pixels and coverage combines with `max`, so the result does not
/// depend on the thread count or on the order segments were added in.
fn splat(layer: &mut Ink, w: usize, h: usize, segs: &[Seg], threads: u32) {
    if segs.is_empty() {
        return;
    }
    let nbands = h.div_ceil(BAND).max(1);
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nbands];
    for (i, s) in segs.iter().enumerate() {
        let lo = (s.a[1].min(s.b[1]) - s.hw - 1.0).floor();
        let hi = (s.a[1].max(s.b[1]) + s.hw + 1.0).ceil();
        if !(lo.is_finite() && hi.is_finite()) {
            continue;
        }
        let b0 = ((lo as i64).max(0) as usize) / BAND;
        let b1 = ((hi as i64).max(0) as usize).min(h.saturating_sub(1)) / BAND;
        for b in b0..=b1.min(nbands - 1) {
            bins[b].push(i as u32);
        }
    }
    layer.any = true;
    par_bands2(&mut layer.cov, &mut layer.z, w, BAND, threads, |y0, cov, zb| {
        let rows = cov.len() / w;
        for &si in &bins[y0 / BAND] {
            splat_one(&segs[si as usize], cov, zb, w, y0, rows);
        }
    });
}

/// Analytic round-capped capsule coverage of one segment into a band.
fn splat_one(s: &Seg, cov: &mut [f32], zb: &mut [f32], w: usize, y0: usize, rows: usize) {
    let (ax, ay, az) = (s.a[0], s.a[1], s.a[2]);
    let (dx, dy, dz) = (s.b[0] - ax, s.b[1] - ay, s.b[2] - az);
    let len2 = dx * dx + dy * dy;
    let inv = if len2 > 1e-12 { 1.0 / len2 } else { 0.0 };
    let reach = s.hw + 1.0;
    let (x0, x1) = (s.a[0].min(s.b[0]) - reach, s.a[0].max(s.b[0]) + reach);
    let (ylo, yhi) = (s.a[1].min(s.b[1]) - reach, s.a[1].max(s.b[1]) + reach);
    if !(x0.is_finite() && x1.is_finite() && ylo.is_finite() && yhi.is_finite()) {
        return;
    }
    let px0 = (x0.floor() as i64).max(0) as usize;
    let px1 = ((x1.ceil() as i64).max(0) as usize).min(w.saturating_sub(1));
    let py0 = ((ylo.floor() as i64).max(0) as usize).max(y0);
    let py1 = ((yhi.ceil() as i64).max(0) as usize).min(y0 + rows - 1);
    if px0 > px1 || py0 > py1 {
        return;
    }
    for y in py0..=py1 {
        let fy = y as f32 + 0.5;
        let row = (y - y0) * w;
        for x in px0..=px1 {
            let fx = x as f32 + 0.5;
            let t = if inv > 0.0 { (((fx - ax) * dx + (fy - ay) * dy) * inv).clamp(0.0, 1.0) } else { 0.0 };
            let (qx, qy) = (ax + dx * t, ay + dy * t);
            let d = ((fx - qx).powi(2) + (fy - qy).powi(2)).sqrt();
            let c = (s.hw + 0.5 - d).clamp(0.0, 1.0) * s.alpha;
            if c <= 0.0 {
                continue;
            }
            let i = row + x;
            let z = az + dz * t;
            if c > cov[i] || (c == cov[i] && z < zb[i]) {
                cov[i] = c;
                zb[i] = z;
            }
        }
    }
}

/// Composites one finished ink layer into `frame.display`, applying the soft
/// depth test and, when `oitb` is given, the translucent attenuation.
fn blend(frame: &mut Frame, ctx: &Ctx<'_>, layer: &Ink, color: [f32; 3], oitb: Option<&oit::Buffers>) {
    if !layer.any {
        return;
    }
    let (w, h) = (frame.w as usize, frame.h as usize);
    let diag = ctx.scene.diag;
    let glass = oitb.map(|b| !b.is_empty()).unwrap_or(false);
    let (depth, cov, zb) = (&frame.depth, &layer.cov, &layer.z);
    par_bands2(&mut frame.display, &mut frame.alpha, w, BAND, ctx.threads, |y0, band, alphas| {
        for (k, (px, pa)) in band.iter_mut().zip(alphas.iter_mut()).enumerate() {
            let i = y0 * w + k;
            let c = cov[i];
            if c <= 0.0 {
                continue;
            }
            let z = zb[i];
            let a = c * visibility(depth, w, h, i, z, diag);
            if a <= 0.0 {
                continue;
            }
            let col = match oitb {
                Some(b) if glass => mul(color, oit::transmittance_at(b, i, z)),
                _ => color,
            };
            *px = lerp3(*px, col, a);
            *pa += (1.0 - *pa) * a;
        }
    });
}

/// Soft depth test with a slope-scaled bias: ink at depth `z` is fully
/// visible while it is in front of (or on) the surface in the G-buffer, and
/// fades out once it is behind it by more than the local per-pixel depth
/// gradient, so a line lying on its own surface neither z-fights nor floats.
#[inline]
fn visibility(depth: &[f32], w: usize, h: usize, i: usize, z: f32, diag: f32) -> f32 {
    let d = depth[i];
    if !d.is_finite() {
        return 1.0;
    }
    let (x, y) = (i % w, i / w);
    let mut slope = 0.0f32;
    if x > 0 && depth[i - 1].is_finite() {
        slope = slope.max((depth[i - 1] - d).abs());
    }
    if x + 1 < w && depth[i + 1].is_finite() {
        slope = slope.max((depth[i + 1] - d).abs());
    }
    if y > 0 && depth[i - w].is_finite() {
        slope = slope.max((depth[i - w] - d).abs());
    }
    if y + 1 < h && depth[i + w].is_finite() {
        slope = slope.max((depth[i + w] - d).abs());
    }
    let slope = slope.min(0.006 * diag);
    let bias = 0.0008 * diag + slope;
    1.0 - smoothstep(bias, bias + 0.0008 * diag + slope, z - d)
}

/// Anti-aliased, depth-tested segment straight into `frame.display`, with the
/// same coverage rule the layered path uses. `depth_tol` is the extra depth
/// slack, in world units, on top of the slope-scaled bias. The overlay itself
/// goes through the layered path ([`splat`] then [`blend`]); this is the
/// single-segment entry point, used by the tests.
#[allow(clippy::too_many_arguments, dead_code)]
pub(crate) fn draw_segment(
    frame: &mut Frame,
    a: [f32; 3],
    b: [f32; 3],
    width_px: f32,
    ink: [f32; 3],
    alpha: f32,
    depth_tol: f32,
    oitb: Option<&oit::Buffers>,
) {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let seg = Seg { a, b, hw: width_px.max(0.2) * 0.5, alpha };
    let mut cov = vec![0.0f32; w * h];
    let mut zb = vec![f32::INFINITY; w * h];
    splat_one(&seg, &mut cov, &mut zb, w, 0, h);
    for i in 0..w * h {
        let c = cov[i];
        if c <= 0.0 {
            continue;
        }
        let z = zb[i] - depth_tol;
        let a = c * visibility(&frame.depth, w, h, i, z, 1.0);
        if a <= 0.0 {
            continue;
        }
        let col = match oitb {
            Some(buf) => mul(ink, oit::transmittance_at(buf, i, z)),
            None => ink,
        };
        frame.display[i] = lerp3(frame.display[i], col, a);
        frame.alpha[i] += (1.0 - frame.alpha[i]) * a;
    }
}

/// Rounds a raw spacing to 1, 2 or 5 times a power of ten.
pub(crate) fn nice_step(raw: f64) -> f64 {
    let p = 10f64.powf(raw.log10().floor());
    let m = raw / p;
    let n = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    n * p
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::*;
    use super::*;
    use crate::context::Material;
    use crate::geom::Plane;
    use crate::sketch::{Loop, P2, Sketch};
    use crate::tessellate::Mesh;

    fn white_frame(w: u32, h: u32) -> Frame {
        let mut f = Frame::new(w, h);
        f.display.fill([1.0, 1.0, 1.0]);
        f
    }

    /// Ink coverage of a pixel after [`draw_segment`] with black ink on white.
    fn cov_at(f: &Frame, x: u32, y: u32) -> f32 {
        1.0 - f.display[f.idx(x, y)][0]
    }

    /// A 1-pixel line centred on a row of pixel centres covers that row and
    /// nothing else; centred on the boundary it splits evenly between the two
    /// rows. This is the whole point of analytic coverage.
    #[test]
    fn segment_coverage_is_analytic() {
        let mut f = white_frame(48, 24);
        draw_segment(&mut f, [4.0, 10.5, 1.0], [40.0, 10.5, 1.0], 1.0, [0.0; 3], 1.0, 0.0, None);
        for x in 6..38 {
            assert!(cov_at(&f, x, 10) > 0.95, "x={x} core coverage {}", cov_at(&f, x, 10));
            assert!(cov_at(&f, x, 9) < 0.05, "x={x} row above {}", cov_at(&f, x, 9));
            assert!(cov_at(&f, x, 11) < 0.05, "x={x} row below {}", cov_at(&f, x, 11));
        }
        let mut f = white_frame(48, 24);
        draw_segment(&mut f, [4.0, 10.0, 1.0], [40.0, 10.0, 1.0], 1.0, [0.0; 3], 1.0, 0.0, None);
        for x in 6..38 {
            for y in [9, 10] {
                let c = cov_at(&f, x, y);
                assert!((0.45..=0.55).contains(&c), "x={x} y={y} coverage {c}");
            }
        }
    }

    /// A diagonal is continuous (no gaps) and lays down about as much ink as
    /// its length times its width.
    #[test]
    fn diagonal_segments_are_continuous() {
        let mut f = white_frame(64, 64);
        draw_segment(&mut f, [8.0, 8.0, 1.0], [56.0, 56.0, 1.0], 1.2, [0.0; 3], 1.0, 0.0, None);
        let total: f32 = (0..64 * 64).map(|i| 1.0 - f.display[i][0]).sum();
        let len = (48.0f32 * 48.0 * 2.0).sqrt();
        assert!(total > len * 1.2 * 0.75 && total < len * 1.2 * 1.35, "ink {total} for length {len}");
        for y in 10..54 {
            let row: f32 = (0..64).map(|x| cov_at(&f, x, y)).sum();
            assert!(row > 0.9, "row {y} has only {row} ink: the line has a gap");
        }
    }

    fn cylinder_body() -> (Mesh, Edges, Vec<f64>) {
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 10.0));
        let solid = crate::kernel::extrude(&s, 20.0).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&solid, 0.05).unwrap();
        (
            mesh,
            topo.edges.iter().map(|e| e.polyline.clone()).collect(),
            topo.edges.iter().map(|e| e.dihedral_degrees).collect(),
        )
    }

    fn line_opts(edges: EdgeMode, crease: f32) -> RenderOptions {
        RenderOptions {
            width: 200,
            height: 160,
            edges,
            edge_crease_degrees: crease,
            draw_silhouettes: false,
            draw_grid: false,
            samples: 1,
            threads: 1,
            ..Default::default()
        }
    }

    /// Ink laid down by the overlay, per pixel, against the same render with
    /// no overlay at all.
    fn ink_map(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> (Vec<f32>, Frame) {
        let bare = render_frame(items, cam, &RenderOptions { edges: EdgeMode::Off, draw_grid: false, ..opts.clone() });
        let inked = render_frame(items, cam, opts);
        let d: Vec<f32> = (0..bare.display.len())
            .map(|i| (luminance(bare.display[i]) - luminance(inked.display[i])).max(0.0))
            .collect();
        (d, bare)
    }

    /// A tessellated cylinder is split into two half-cylinder faces; the seam
    /// between them is tangent and must not be drawn, while the rim where the
    /// side meets the top is a 90 degree crease and must be.
    #[test]
    fn the_crease_filter_hides_tangent_seams_and_keeps_rims() {
        let (mesh, edges, creases) = cylinder_body();
        let mat = Material::default();
        let item = |c: &'static [f64]| Item { edge_dihedrals: c, ..Item::new(&mesh, &mat, &edges) };
        let _ = item;
        let its = [Item { edge_dihedrals: &creases, ..Item::new(&mesh, &mat, &edges) }];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let (all, bare) = ink_map(&its, &cam, &line_opts(EdgeMode::All, 30.0));
        let (creased, _) = ink_map(&its, &cam, &line_opts(EdgeMode::Creases, 30.0));
        let (steep, _) = ink_map(&its, &cam, &line_opts(EdgeMode::Creases, 120.0));
        // Side of the cylinder, away from both rims; and the top rim.
        let side = |i: usize| bare.coverage[i] & COV_OPAQUE != 0 && (3.0..17.0).contains(&bare.pos[i][2]);
        let rim = |i: usize| bare.coverage[i] & COV_OPAQUE != 0 && bare.pos[i][2] > 19.5;
        let count = |m: &[f32], f: &dyn Fn(usize) -> bool| m.iter().enumerate().filter(|(i, v)| **v > 0.02 && f(*i)).count();
        let (side_all, side_creased) = (count(&all, &side), count(&creased, &side));
        let (rim_all, rim_creased, rim_steep) = (count(&all, &rim), count(&creased, &rim), count(&steep, &rim));
        assert!(side_all > 40, "every seam drawn should ink the cylinder side, got {side_all}");
        assert!(
            (side_creased as f32) < 0.1 * side_all as f32,
            "the crease filter left {side_creased} of {side_all} seam pixels"
        );
        assert!(rim_all > 40 && rim_creased as f32 > 0.7 * rim_all as f32, "rim {rim_creased} of {rim_all}");
        assert!(
            (rim_steep as f32) < 0.1 * rim_creased as f32,
            "a 120 degree threshold should drop the 90 degree rim, got {rim_steep} of {rim_creased}"
        );
    }

    /// Without topology the overlay estimates the crease angle from the
    /// G-buffer normals, and must reach the same conclusion.
    #[test]
    fn the_crease_estimator_hides_tangent_seams_without_topology() {
        let (mesh, edges, _) = cylinder_body();
        let mat = Material::default();
        let its = [Item::new(&mesh, &mat, &edges)];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let (all, bare) = ink_map(&its, &cam, &line_opts(EdgeMode::All, 30.0));
        let (creased, _) = ink_map(&its, &cam, &line_opts(EdgeMode::Creases, 30.0));
        let side = |i: usize| bare.coverage[i] & COV_OPAQUE != 0 && (3.0..17.0).contains(&bare.pos[i][2]);
        let count = |m: &[f32], f: &dyn Fn(usize) -> bool| m.iter().enumerate().filter(|(i, v)| **v > 0.02 && f(*i)).count();
        let (side_all, side_creased) = (count(&all, &side), count(&creased, &side));
        assert!(side_all > 40);
        assert!((side_creased as f32) < 0.35 * side_all as f32, "estimator left {side_creased} of {side_all}");
    }

    /// Line width is in pixels and is not affected by the sample count: the
    /// total ink must be the same at 1, 4 and 8 samples per pixel.
    #[test]
    fn line_weight_does_not_depend_on_the_sample_count() {
        let (mesh, edges, creases) = cylinder_body();
        let mat = Material::default();
        let its = [Item { edge_dihedrals: &creases, ..Item::new(&mesh, &mat, &edges) }];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mut totals = Vec::new();
        for samples in [1, 4, 8] {
            let opts = RenderOptions { samples, ..line_opts(EdgeMode::Creases, 30.0) };
            let (ink, _) = ink_map(&its, &cam, &opts);
            totals.push(ink.iter().sum::<f32>());
        }
        let (lo_t, hi_t) = (totals.iter().cloned().fold(f32::MAX, f32::min), totals.iter().cloned().fold(0.0, f32::max));
        assert!(hi_t < lo_t * 1.15, "ink totals vary with sample count: {totals:?}");
    }

    /// A quad facing the top camera at height `z`, spanning `x0..x1`.
    fn plate(z: f32, x0: f32, x1: f32) -> Mesh {
        let p = [[x0, -2.0, z], [x1, -2.0, z], [x1, 3.0, z], [x0, 3.0, z]];
        let mut m = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
        for v in p {
            m.positions.extend_from_slice(&v);
            m.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        m.face_ids.extend_from_slice(&[0, 0]);
        m
    }

    /// Ink behind glass must still be visible, but dimmer than the same ink
    /// in the open.
    #[test]
    fn edges_behind_glass_are_visible_but_attenuated() {
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let glass_mat = Material { base_color: [0.9, 0.95, 1.0], ..Material::default() };
        let glass = plate(2.0, 0.5, 4.0);
        let mut cam = Camera::preset("top", Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0), 0.4);
        cam.orthographic = true;
        let opts = RenderOptions {
            width: 120,
            height: 120,
            edges: EdgeMode::All,
            draw_silhouettes: false,
            draw_grid: false,
            samples: 1,
            threads: 1,
            ..Default::default()
        };
        let bare = [Item::new(&mesh, &mat, &edges)];
        let with_glass =
            [Item::new(&mesh, &mat, &edges), Item { opacity: Some(0.35), ..Item::new(&glass, &glass_mat, &[]) }];
        let open = render_frame(&bare, &cam, &opts);
        let under = render_frame(&with_glass, &cam, &opts);
        let no_ink = render_frame(&with_glass, &cam, &RenderOptions { edges: EdgeMode::Off, ..opts.clone() });
        // Edge pixels on the glass side of the image (world x > 0.5).
        let mut open_l = Vec::new();
        let mut under_l = Vec::new();
        let mut glass_l = Vec::new();
        for i in 0..open.display.len() {
            let inked = luminance(no_ink.display[i]) - luminance(under.display[i]);
            let x = open.pos[i][0];
            if open.coverage[i] & COV_OPAQUE == 0 || x < 0.6 {
                continue;
            }
            if inked > 0.01 {
                open_l.push(luminance(open.display[i]));
                under_l.push(luminance(under.display[i]));
            } else if under.coverage[i] & COV_TRANSLUCENT != 0 {
                glass_l.push(luminance(under.display[i]));
            }
        }
        assert!(open_l.len() > 20, "found only {} inked pixels under the glass", open_l.len());
        assert!(!glass_l.is_empty());
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let (open_m, under_m, glass_m) = (mean(&open_l), mean(&under_l), mean(&glass_l));
        assert!(under_m > open_m, "ink under glass ({under_m}) should be lighter than in the open ({open_m})");
        assert!(under_m < glass_m, "ink under glass ({under_m}) should still be darker than the glass ({glass_m})");
    }

    /// The infinite grid fades out with distance, so the far half of the
    /// image carries less grid ink than the near half.
    #[test]
    fn the_grid_fades_into_the_distance() {
        let (mesh, edges) = l_block();
        let mat = Material::default();
        let its = [Item::new(&mesh, &mat, &edges)];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.6);
        let opts = RenderOptions {
            width: 240,
            height: 180,
            edges: EdgeMode::Off,
            grid: GridMode::Infinite,
            samples: 1,
            threads: 1,
            ..Default::default()
        };
        let f = render_frame(&its, &cam, &opts);
        let bg = luminance(opts.background);
        let band = |y0: u32, y1: u32| {
            let mut sum = 0.0f32;
            for y in y0..y1 {
                for x in 0..opts.width {
                    let i = f.idx(x, y);
                    if f.coverage[i] == 0 {
                        sum += (bg - luminance(f.display[i])).max(0.0);
                    }
                }
            }
            sum / ((y1 - y0) * opts.width) as f32
        };
        let (far, near) = (band(0, 40), band(140, 180));
        assert!(near > 0.001, "no grid ink near the camera: {near}");
        assert!(far < near * 0.6, "grid does not fade with distance: far {far} near {near}");
    }

    /// Selection ink wins over feature edges and keeps the highlight hue.
    #[test]
    fn highlight_edges_are_drawn_last_and_keep_their_colour() {
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let hl: Edges = vec![edges[4].clone()];
        let its = [Item { highlight_edges: &hl, ..Item::new(&mesh, &mat, &edges) }];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.3);
        let opts = RenderOptions { width: 160, height: 120, draw_grid: false, samples: 1, threads: 1, ..Default::default() };
        let f = render_frame(&its, &cam, &opts);
        let warm = f
            .display
            .iter()
            .filter(|c| c[0] > 0.5 && c[0] > c[2] * 2.0 && c[1] > c[2])
            .count();
        assert!(warm > 20, "highlight ink not found: {warm} warm pixels");
    }

    /// The whole overlay, translucency included, is thread invariant.
    #[test]
    fn the_overlay_is_thread_invariant() {
        let (mesh, edges, creases) = cylinder_body();
        let mat = Material::default();
        let glass = plate(25.0, -12.0, 12.0);
        let its = [
            Item { edge_dihedrals: &creases, ..Item::new(&mesh, &mat, &edges) },
            Item { opacity: Some(0.4), ..Item::new(&glass, &mat, &[]) },
        ];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let opts = RenderOptions { width: 200, height: 150, ..Default::default() };
        let a = render(&its, &cam, &RenderOptions { threads: 1, ..opts.clone() });
        let b = render(&its, &cam, &RenderOptions { threads: 4, ..opts.clone() });
        let c = render(&its, &cam, &RenderOptions { threads: 8, ..opts });
        assert_eq!(fnv1a64(&a.rgba), fnv1a64(&b.rgba));
        assert_eq!(fnv1a64(&a.rgba), fnv1a64(&c.rgba));
    }

    #[test]
    fn nice_step_rounds_to_one_two_or_five() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(2.9), 2.0);
        assert_eq!(nice_step(4.0), 5.0);
        assert_eq!(nice_step(23.0), 20.0);
    }
}
