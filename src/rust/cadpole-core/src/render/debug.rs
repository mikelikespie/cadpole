//! Debug views: renders that show the *mesh* rather than the part.
//!
//! A normal render answers "what does this look like"; these answer "what
//! did the tessellator and the booleans actually produce". Every one of them
//! is a question that has otherwise meant a rebuild with trace prints: did a
//! boolean keep its faces or shatter them into one-triangle groups, is that
//! facet a real crease or a seam the crease rule should have smoothed, is
//! the artefact in the glass render a defective triangle or real geometry
//! seen through refraction, and why is this cone band fifteen thousand
//! triangles.
//!
//! The mechanism is the one the analysis views use: a colour per triangle in
//! [`super::Item::tri_colors`], read by the rasterizer's shading, plus a
//! flat headlight in place of the studio rig so the colour *is* the reading
//! (`shading::shade_debug`). `seams` and `wire` are overlays rather than
//! colours and live in `lines.rs`, next to the crease filter they explain.
//! What is here is the vocabulary and the two painters that need only a
//! mesh: the face palette and the defect painter.

use crate::solid;
use crate::tessellate::Mesh;

/// Which debug view to draw. `cadpole render --debug <mode>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugView {
    /// Every topology face group in its own colour, flat-lit. A boolean that
    /// shattered its faces shows as noise; one that kept them shows as a
    /// handful of clean patches.
    Faces,
    /// The shading normal as a colour, `n * 0.5 + 0.5`, unlit. A surface
    /// that should read smooth and does not shows its facets at once.
    Normals,
    /// Grey body with the triangles `solid::validate` flags painted:
    /// crossings red, slivers orange, folds magenta.
    Defects,
    /// Flat grey with the feature edges classified by the crease rule:
    /// creases (dihedral at or above the threshold) in black, seams the rule
    /// smoothed over (below it) in blue.
    Seams,
    /// Every triangle edge over a faint body, for tessellation density.
    Wire,
}

impl DebugView {
    pub fn parse(s: &str) -> Result<DebugView, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "faces" | "face" => Ok(DebugView::Faces),
            "normals" | "normal" => Ok(DebugView::Normals),
            "defects" | "defect" => Ok(DebugView::Defects),
            "seams" | "seam" | "creases" => Ok(DebugView::Seams),
            "wire" | "wireframe" => Ok(DebugView::Wire),
            other => Err(format!(
                "unknown debug view {other:?}; the views are faces, normals, defects, seams and wire"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DebugView::Faces => "faces",
            DebugView::Normals => "normals",
            DebugView::Defects => "defects",
            DebugView::Seams => "seams",
            DebugView::Wire => "wire",
        }
    }

    /// Every view, in the order the documentation lists them.
    pub fn all() -> [DebugView; 5] {
        [DebugView::Faces, DebugView::Normals, DebugView::Defects, DebugView::Seams, DebugView::Wire]
    }
}

// ---------------------------------------------------------------------------
// Colours. Display-linear, like everything the shading writes.

/// The body under the defect paint and the seam overlay: a mid grey that
/// neither red, orange, magenta, black nor blue can be mistaken for.
pub const BODY_GREY: [f32; 3] = [0.60, 0.61, 0.63];
/// The body under the wireframe, lighter so dark ink reads on it.
pub const WIRE_BODY: [f32; 3] = [0.84, 0.85, 0.86];
/// Two triangles that pass through each other.
pub const CROSSING: [f32; 3] = [0.90, 0.06, 0.06];
/// A triangle thinner than the tolerance.
pub const SLIVER: [f32; 3] = [0.98, 0.52, 0.04];
/// Two adjacent triangles facing opposite ways.
pub const FOLD: [f32; 3] = [0.88, 0.08, 0.84];
/// `seams`: a crease the rule keeps.
pub const CREASE_INK: [f32; 3] = [0.015, 0.015, 0.018];
/// `seams`: a seam the rule smoothed over.
pub const SEAM_INK: [f32; 3] = [0.08, 0.32, 0.95];
/// `wire`: every triangle edge.
pub const WIRE_INK: [f32; 3] = [0.06, 0.08, 0.16];

/// The colour of topology face `face` of item `item`.
///
/// Hue steps by the golden ratio, which is the sequence that keeps every
/// prefix of indices as far apart on the wheel as it can be, so faces 0..12
/// of a bracket are twelve clearly different colours and the same model
/// colours the same way in every render. The item index shifts the whole
/// wheel so two bodies' face 0 differ too. Saturation and value are fixed:
/// the reading is *which patch*, and a brightness ramp would be mistaken
/// for lighting.
pub fn face_color(face: u32, item: usize) -> [f32; 3] {
    const GOLDEN: f64 = 0.618_033_988_749_895;
    let h = ((face as f64 + 1.0) * GOLDEN + item as f64 * 0.271).fract() as f32;
    hsv(h, 0.62, 0.90)
}

/// One colour per triangle from [`face_color`] of its face id.
pub fn face_colors(mesh: &Mesh, item: usize) -> Vec<[f32; 3]> {
    (0..mesh.triangle_count()).map(|t| face_color(mesh.face_ids.get(t).copied().unwrap_or(0), item)).collect()
}

/// Number of distinct face ids a mesh carries, for the summary line.
pub fn face_count(mesh: &Mesh) -> usize {
    let mut seen: Vec<u32> = mesh.face_ids.clone();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match (i as i32).rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

// ---------------------------------------------------------------------------
// Defects.

/// What the defect painter found, for the legend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefectCounts {
    /// Triangles that pass through another (each side of a pair counts).
    pub crossings: usize,
    /// Triangles whose smallest altitude is below the tolerance.
    pub slivers: usize,
    /// Triangles that share an edge with one facing the opposite way.
    pub folds: usize,
}

impl DefectCounts {
    pub fn add(&mut self, o: &DefectCounts) {
        self.crossings += o.crossings;
        self.slivers += o.slivers;
        self.folds += o.folds;
    }

    /// The one-line legend: what each colour means and how many there are.
    pub fn summary(&self) -> String {
        format!(
            "defects: {} crossing triangles (red), {} slivers (orange), {} folds (magenta)",
            self.crossings, self.slivers, self.folds
        )
    }
}

/// Paint the triangles [`solid::validate`] would flag over a grey body.
///
/// Crossings and folds come from the validator itself, run on the welded
/// mesh the way `cadpole check` runs it, and are mapped back to the triangles
/// the renderer draws through [`Mesh::welded_with_map`] — a fold being what
/// [`solid::Fold`] says it is: two triangles on one edge whose normals are
/// antiparallel to within [`solid::FOLD_DOT`], the same pairs `cadpole inspect`
/// counts. Slivers are computed here: the validator reports a degenerate
/// triangle as a position, not an index, and its notion is "no area at all",
/// whereas the triangle that makes a boolean or a normal misbehave still has
/// area — it is merely thinner than anything the tessellation tolerance
/// could have asked for. So a sliver is a triangle whose smallest altitude
/// is below `sliver_altitude`.
pub fn defect_colors(mesh: &Mesh, sliver_altitude: f64) -> (Vec<[f32; 3]>, DefectCounts) {
    let n = mesh.triangle_count();
    let mut colors = vec![BODY_GREY; n];
    let mut counts = DefectCounts::default();
    if n == 0 {
        return (colors, counts);
    }
    // The validator's pair limit exists so a check nobody asked for does not
    // cost a minute; this one was asked for by name, and the impeller (400k
    // triangles) is over the limit and is exactly the mesh in question.
    let opts = solid::SolidOptions { pair_limit: usize::MAX, ..solid::SolidOptions::default() };
    let (welded, origin) = mesh.welded_with_map(opts.weld_eps);

    // Slivers, on the unwelded mesh: a triangle the weld collapsed is the
    // thinnest kind there is and still has to be painted.
    for t in 0..n {
        let v = solid::tri(mesh, t);
        let (_, area) = solid::tri_normal_area(&v);
        let longest = (v[1] - v[0]).length().max((v[2] - v[1]).length()).max((v[0] - v[2]).length());
        let altitude = if longest > 0.0 { 2.0 * area / longest } else { 0.0 };
        if altitude < sliver_altitude {
            colors[t] = SLIVER;
            counts.slivers += 1;
        }
    }

    // Folds and crossings, from the one report `cadpole check` and `cadpole
    // inspect` read; an exact fin is reported once per shared edge, so the
    // triangles are marked and then counted, not the report's entries.
    let report = solid::validate_welded(&welded, n - welded.triangle_count(), &opts);
    let mut folded = vec![false; welded.triangle_count()];
    for f in &report.folds {
        folded[f.tris.0 as usize] = true;
        folded[f.tris.1 as usize] = true;
    }
    for (t, &f) in folded.iter().enumerate() {
        if f {
            colors[origin[t] as usize] = FOLD;
            counts.folds += 1;
        }
    }

    // Crossings last, so a triangle that is both a sliver and a crossing
    // reads as the worse of the two.
    let mut crossing = vec![false; welded.triangle_count()];
    for p in &report.crossings {
        crossing[p.tris.0 as usize] = true;
        crossing[p.tris.1 as usize] = true;
    }
    for (t, &c) in crossing.iter().enumerate() {
        if c {
            colors[origin[t] as usize] = CROSSING;
            counts.crossings += 1;
        }
    }
    (colors, counts)
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{Camera, EdgeMode, Image, Item, RenderOptions, render};
    use super::*;
    use crate::context::Material;
    use crate::geom::Vec3;
    use crate::solid::fixtures;

    /// A debug render with nothing but the view: no grid, no silhouettes,
    /// edges only when asked, one thread so a failure reproduces.
    fn debug_opts(view: DebugView, edges: bool) -> RenderOptions {
        let mut o = RenderOptions {
            width: 240,
            height: 180,
            draw_grid: false,
            draw_edges: edges,
            draw_silhouettes: false,
            threads: 1,
            ..Default::default()
        };
        o.set_debug(view);
        o
    }

    /// The distinct colours in an image, merging any two within `tol` per
    /// channel: the resolve dithers by up to half a level, so one flat
    /// colour can land on two neighbouring bytes.
    fn distinct_colors(img: &Image, tol: i32) -> Vec<[u8; 3]> {
        let mut out: Vec<[u8; 3]> = Vec::new();
        for px in img.rgba.chunks_exact(4) {
            let c = [px[0], px[1], px[2]];
            if !out.iter().any(|o| (0..3).all(|k| (o[k] as i32 - c[k] as i32).abs() <= tol)) {
                out.push(c);
            }
        }
        out
    }

    fn background(opts: &RenderOptions) -> [u8; 3] {
        opts.background.map(super::super::to_srgb8)
    }

    fn pixel(img: &Image, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }

    /// Six faces, six colours: three from each of two opposite corners,
    /// every one different from the others and from the background, and no
    /// seventh colour anywhere — the headlight is directional, so a flat
    /// face is one colour under perspective too.
    #[test]
    fn faces_mode_gives_a_box_exactly_six_colours() {
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let colors = face_colors(&mesh, 0);
        let items = [Item { tri_colors: &colors, ..Item::new(&mesh, &mat, &edges) }];
        let opts = debug_opts(DebugView::Faces, false);
        let (lo, hi) = mesh.bbox().unwrap();
        let mut seen = Vec::new();
        for preset in ["iso", "az=225,el=-30"] {
            let img = render(&items, &Camera::preset(preset, lo, hi, 0.3), &opts);
            let here = distinct_colors(&img, 2);
            assert_eq!(here.len(), 4, "{preset}: three faces and the background, got {here:?}");
            seen.extend(here);
        }
        let bg = background(&opts);
        seen.retain(|c| *c != bg);
        let mut merged: Vec<[u8; 3]> = Vec::new();
        for c in seen {
            if !merged.iter().any(|o| (0..3).all(|k| (o[k] as i32 - c[k] as i32).abs() <= 2)) {
                merged.push(c);
            }
        }
        assert_eq!(merged.len(), 6, "six faces, six colours: {merged:?}");
    }

    /// A 48-gon barrel about Z; `smooth` gives every vertex the analytic
    /// normal, otherwise each facet its own.
    fn barrel(smooth: bool) -> Mesh {
        let n = 48u32;
        let mut m = Mesh::default();
        for i in 0..n {
            let (a0, a1) = (i as f32 / n as f32 * std::f32::consts::TAU, (i + 1) as f32 / n as f32 * std::f32::consts::TAU);
            let flat = [(a0 + a1).cos() * 0.5, (a0 + a1).sin() * 0.5, 0.0];
            let flat = {
                let l = (flat[0] * flat[0] + flat[1] * flat[1]).sqrt();
                [flat[0] / l, flat[1] / l, 0.0]
            };
            let base = (m.positions.len() / 3) as u32;
            for (a, z) in [(a0, 0.0), (a1, 0.0), (a1, 20.0), (a0, 20.0)] {
                m.positions.extend_from_slice(&[10.0 * a.cos(), 10.0 * a.sin(), z]);
                m.normals.extend_from_slice(&if smooth { [a.cos(), a.sin(), 0.0] } else { flat });
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            m.face_ids.extend_from_slice(&[0, 0]);
        }
        m
    }

    /// Across the middle of a smooth barrel no two neighbouring columns
    /// differ by more than a few levels; with flat normals the same barrel
    /// jumps at every facet. A box shows three flat regions.
    #[test]
    fn normals_mode_shows_smooth_as_smooth_and_flat_as_flat() {
        let mat = Material::default();
        // Wide enough that the barrel covers well over a hundred columns:
        // the preset camera fits a bounding sphere, which leaves a cylinder
        // small in the frame.
        let opts = RenderOptions { width: 320, height: 240, ..debug_opts(DebugView::Normals, false) };
        let bg = background(&opts);
        let steps = |mesh: &Mesh| {
            let items = [Item::new(mesh, &mat, &[])];
            let (lo, hi) = mesh.bbox().unwrap();
            let mut cam = Camera::preset("front", lo, hi, 0.05);
            cam.orthographic = true;
            let img = render(&items, &cam, &opts);
            let y = opts.height / 2;
            let covered: Vec<u32> = (0..opts.width).filter(|&x| pixel(&img, x, y) != bg).collect();
            let (x0, x1) = (covered[0], *covered.last().unwrap());
            assert!(x1 - x0 > 100, "the barrel spans {x0}..{x1}");
            // The middle half: at the limbs the normal turns fast per pixel
            // whatever the mesh says.
            let (a, b) = (x0 + (x1 - x0) / 4, x1 - (x1 - x0) / 4);
            (a..b)
                .map(|x| {
                    let (p, q) = (pixel(&img, x, y), pixel(&img, x + 1, y));
                    (0..3).map(|k| (p[k] as i32 - q[k] as i32).abs()).max().unwrap()
                })
                .max()
                .unwrap()
        };
        let smooth = steps(&barrel(true));
        let faceted = steps(&barrel(false));
        assert!(smooth <= 6, "a smooth barrel steps by {smooth} levels between columns");
        assert!(faceted > 12, "a faceted barrel should show its facets; largest step {faceted}");

        let (mesh, edges) = unit_box();
        let items = [Item::new(&mesh, &mat, &edges)];
        let (lo, hi) = mesh.bbox().unwrap();
        let img = render(&items, &Camera::preset("iso", lo, hi, 0.3), &opts);
        let mut colors = distinct_colors(&img, 2);
        colors.retain(|c| *c != bg);
        assert_eq!(colors.len(), 3, "three visible faces, three flat normals: {colors:?}");
    }

    fn red(img: &Image) -> usize {
        img.rgba.chunks_exact(4).filter(|p| p[0] > 150 && p[1] < 70 && p[2] < 70).count()
    }

    #[test]
    fn defects_mode_paints_a_crossing_red_and_a_clean_box_not_at_all() {
        let mat = Material::default();
        let opts = debug_opts(DebugView::Defects, false);
        let paint = |mesh: &Mesh| {
            let (colors, _) = defect_colors(mesh, 1e-3);
            let items = [Item { tri_colors: &colors, ..Item::new(mesh, &mat, &[]) }];
            let (lo, hi) = mesh.bbox().unwrap();
            red(&render(&items, &Camera::preset("iso", lo, hi, 0.3), &opts))
        };
        assert_eq!(paint(&fixtures::box_mesh(10.0, 10.0, 10.0)), 0);
        assert!(paint(&fixtures::overlapping_boxes()) > 50);
    }

    /// Pixels an overlay touched, against the same render with the overlay
    /// off.
    fn ink_pixels(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> usize {
        let bare = render(items, cam, &RenderOptions { draw_edges: false, ..opts.clone() });
        let inked = render(items, cam, opts);
        bare.rgba
            .chunks_exact(4)
            .zip(inked.rgba.chunks_exact(4))
            .filter(|(a, b)| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 3))
            .count()
    }

    /// Every triangle edge is more ink than the feature edges: the box's
    /// three visible faces each gain a diagonal. Both renders draw one-pixel
    /// lines so the comparison is about which lines, not how wide.
    #[test]
    fn wire_mode_inks_more_than_the_plain_render() {
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.3);
        let grey = vec![WIRE_BODY; mesh.triangle_count()];
        let wired = [Item { tri_colors: &grey, ..Item::new(&mesh, &mat, &edges) }];
        let wire = ink_pixels(&wired, &cam, &RenderOptions { edge_width_px: 1.0, ..debug_opts(DebugView::Wire, true) });
        let plain = [Item::new(&mesh, &mat, &edges)];
        let plain_opts = RenderOptions {
            width: 240,
            height: 180,
            draw_grid: false,
            draw_silhouettes: false,
            threads: 1,
            samples: 1,
            edge_width_px: 1.0,
            edges: EdgeMode::All,
            ..Default::default()
        };
        let plain = ink_pixels(&plain, &cam, &plain_opts);
        assert!(plain > 0);
        assert!(wire as f32 > plain as f32 * 1.2, "wire {wire} px of ink against {plain} for the feature edges");
    }

    /// The kernel's cylinder is two half-cylinder patches whose seams are
    /// tangent and whose rims are 90 degree creases: `seams` draws the
    /// former in blue and the latter in black.
    #[test]
    fn seams_mode_draws_tangent_seams_blue_and_creases_black() {
        use crate::geom::Plane;
        use crate::sketch::{Loop, P2, Sketch};
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 10.0));
        let solid = crate::kernel::extrude(&s, 20.0).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&solid, 0.05).unwrap();
        let edges: Vec<Vec<Vec3>> = topo.edges.iter().map(|e| e.polyline.clone()).collect();
        let creases: Vec<f64> = topo.edges.iter().map(|e| e.dihedral_degrees).collect();
        assert!(creases.iter().any(|&d| (0.0..30.0).contains(&d)), "no tangent seam to draw: {creases:?}");
        let mat = Material::default();
        let grey = vec![BODY_GREY; mesh.triangle_count()];
        let items = [Item { tri_colors: &grey, edge_dihedrals: &creases, ..Item::new(&mesh, &mat, &edges) }];
        let (lo, hi) = mesh.bbox().unwrap();
        let img = render(&items, &Camera::preset("iso", lo, hi, 0.3), &debug_opts(DebugView::Seams, true));
        let blue = img.rgba.chunks_exact(4).filter(|p| p[2] > 150 && p[0] < 100).count();
        let black = img.rgba.chunks_exact(4).filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40).count();
        assert!(blue > 10, "{blue} blue pixels");
        assert!(black > 10, "{black} black pixels");
    }

    #[test]
    fn every_view_parses_its_own_name() {
        for v in DebugView::all() {
            assert_eq!(DebugView::parse(v.as_str()), Ok(v));
        }
        assert!(DebugView::parse("shaded").is_err());
    }

    /// Thirty consecutive faces are thirty colours a person can tell apart,
    /// and the palette is a pure function of the index.
    #[test]
    fn the_face_palette_is_stable_and_spread_out() {
        let colors: Vec<_> = (0..30).map(|f| face_color(f, 0)).collect();
        for (i, a) in colors.iter().enumerate() {
            assert_eq!(*a, face_color(i as u32, 0));
            for (j, b) in colors.iter().enumerate() {
                if i != j {
                    let d = (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max);
                    assert!(d > 0.04, "faces {i} and {j} are too close: {a:?} vs {b:?}");
                }
            }
        }
        assert_ne!(face_color(0, 0), face_color(0, 1), "two bodies' first faces differ");
    }

    #[test]
    fn a_clean_box_has_no_defects_and_crossing_boxes_do() {
        let (colors, counts) = defect_colors(&fixtures::box_mesh(10.0, 10.0, 10.0), 1e-3);
        assert_eq!(counts, DefectCounts::default());
        assert!(colors.iter().all(|c| *c == BODY_GREY));

        let (colors, counts) = defect_colors(&fixtures::overlapping_boxes(), 1e-3);
        assert!(counts.crossings > 0, "{}", counts.summary());
        assert_eq!(counts.crossings, colors.iter().filter(|c| **c == CROSSING).count());
        assert!(counts.summary().contains("crossing"));
    }

    #[test]
    fn a_zero_area_triangle_is_a_sliver_and_a_doubled_back_one_is_a_fold() {
        let (colors, counts) = defect_colors(&fixtures::box_with_degenerate(10.0, 10.0, 10.0), 1e-3);
        assert_eq!(counts.slivers, 1);
        assert_eq!(*colors.last().unwrap(), SLIVER);

        // A quad folded along its diagonal: two triangles share an edge and
        // face opposite ways.
        let mut m = Mesh::default();
        let (a, b, c) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0));
        fixtures::push_tri(&mut m, a, b, c, 0);
        fixtures::push_tri(&mut m, a, c, b, 0);
        let (colors, counts) = defect_colors(&m, 1e-3);
        assert_eq!(counts.folds, 2, "{}", counts.summary());
        assert!(colors.iter().all(|c| *c == FOLD));
    }
}
