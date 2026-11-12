//! Alpha: what a render without a backdrop writes
//! ([`RenderOptions::transparent`](super::RenderOptions::transparent)).
//!
//! Both CPU backends accumulate *premultiplied* colour and a coverage alpha
//! per pixel, over the same samples and with the same weights, and hand them
//! to [`resolve`]. Premultiplied is the form that averages correctly — a
//! sample that missed the model contributes nothing to either — and the
//! file wants the other form, so the resolve divides: the stored colour is
//! the model's own, never mixed with a background, and an edge pixel
//! composited over any colour lands between that colour and the model's.
//!
//! The GPU backend has no alpha target; [`difference_matte`] recovers the
//! same two quantities from one render over black and one over white.

use super::{Image, from_srgb8, to_srgb8};

/// Straight colour and alpha from a premultiplied pixel.
///
/// Alpha is raised to the largest colour component when the colour exceeds
/// it: a reflection on clear glass is light *added* to what is behind, and
/// a straight-alpha file has no way to say "add" except by being that
/// opaque there. Nothing is lost — `straight * alpha` is still the
/// premultiplied colour that went in.
#[inline]
pub fn unpremultiply(p: [f32; 3], a: f32) -> ([f32; 3], f32) {
    let p = p.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
    let a = if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 };
    let a = a.max(p[0]).max(p[1]).max(p[2]);
    if a <= 0.0 { ([0.0; 3], 0.0) } else { (p.map(|c| (c / a).min(1.0)), a) }
}

/// Averages `n` accumulated premultiplied display-linear samples and their
/// alphas into a straight-alpha sRGB8 image. Colour gets the same 2×2
/// ordered dither as the opaque resolve; alpha is rounded, so 0 and 1 are
/// exact.
pub(crate) fn resolve(premult: &[[f32; 3]], alpha: &[f32], n: u32, w: u32, h: u32) -> Image {
    let inv = 1.0 / n.max(1) as f32;
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    const BAYER: [[f32; 2]; 2] = [[-0.375, 0.125], [0.375, -0.125]];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * w as usize + x;
            let (c, a) = unpremultiply(premult[i].map(|v| v * inv), alpha[i] * inv);
            let a8 = (a * 255.0 + 0.5) as u8;
            if a8 == 0 {
                continue;
            }
            let o = i * 4;
            for k in 0..3 {
                rgba[o + k] = super::aa::dither_srgb8(c[k], BAYER[y & 1][x & 1]);
            }
            rgba[o + 3] = a8;
        }
    }
    Image { width: w, height: h, rgba }
}

/// One pixel as premultiplied display-linear colour and alpha.
#[inline]
fn premultiplied(px: &[u8]) -> [f32; 4] {
    let a = px[3] as f32 / 255.0;
    [from_srgb8(px[0]) * a, from_srgb8(px[1]) * a, from_srgb8(px[2]) * a, a]
}

/// Box-filter `img` down by `k` in each direction, in display-linear light
/// and premultiplied, so a transparent sub-pixel takes no part in the
/// colour. `k` ≤ 1 returns the image as it is; a size that `k` does not
/// divide loses its last partial row and column.
pub fn downsample(img: &Image, k: u32) -> Image {
    if k <= 1 {
        return img.clone();
    }
    let (w, h) = ((img.width / k).max(1), (img.height / k).max(1));
    let mut out = Image { width: w, height: h, rgba: vec![0u8; (w * h * 4) as usize] };
    let inv = 1.0 / (k * k) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut s = [0.0f32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let (sx, sy) = ((x * k + dx).min(img.width - 1), (y * k + dy).min(img.height - 1));
                    let o = ((sy * img.width + sx) * 4) as usize;
                    let p = premultiplied(&img.rgba[o..o + 4]);
                    for c in 0..4 {
                        s[c] += p[c];
                    }
                }
            }
            let (c, a) = unpremultiply([s[0] * inv, s[1] * inv, s[2] * inv], s[3] * inv);
            let o = ((y * w + x) * 4) as usize;
            let a8 = (a * 255.0 + 0.5) as u8;
            if a8 != 0 {
                out.rgba[o] = to_srgb8(c[0]);
                out.rgba[o + 1] = to_srgb8(c[1]);
                out.rgba[o + 2] = to_srgb8(c[2]);
                out.rgba[o + 3] = a8;
            }
        }
    }
    out
}

/// `img` composited over an opaque display-linear colour, in linear light:
/// what a viewer that blends correctly shows. For tests and contact sheets.
pub fn over(img: &Image, bg: [f32; 3]) -> Image {
    let mut out = img.clone();
    for px in out.rgba.chunks_exact_mut(4) {
        let p = premultiplied(px);
        for c in 0..3 {
            px[c] = to_srgb8(p[c] + (1.0 - p[3]) * bg[c]);
        }
        px[3] = 255;
    }
    out
}

/// Alpha from two renders of one scene, over black and over white.
///
/// A renderer that mixes its background in display-linear light writes
/// `premult + (1 − alpha) · background`, so the black render *is* the
/// premultiplied colour and the white one exceeds it by `1 − alpha`. That
/// is exact coverage from the renderer's own anti-aliasing, with no
/// threshold and no colour to key on; the price is two renders and the 8
/// bits the two were quantised to.
pub fn difference_matte(black: &Image, white: &Image) -> Image {
    let mut out = Image { width: black.width, height: black.height, rgba: vec![0u8; black.rgba.len()] };
    if black.width != white.width || black.height != white.height {
        return out;
    }
    for ((o, b), w) in out.rgba.chunks_exact_mut(4).zip(black.rgba.chunks_exact(4)).zip(white.rgba.chunks_exact(4)) {
        let p = [from_srgb8(b[0]), from_srgb8(b[1]), from_srgb8(b[2])];
        let d = (0..3).map(|c| from_srgb8(w[c]) - p[c]).sum::<f32>() / 3.0;
        let (c, a) = unpremultiply(p, 1.0 - d);
        let a8 = (a * 255.0 + 0.5) as u8;
        if a8 != 0 {
            o[0] = to_srgb8(c[0]);
            o[1] = to_srgb8(c[1]);
            o[2] = to_srgb8(c[2]);
            o[3] = a8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpremultiply_divides_and_keeps_the_premultiplied_colour() {
        let (c, a) = unpremultiply([0.2, 0.1, 0.05], 0.25);
        assert_eq!(a, 0.25);
        for (got, want) in c.iter().zip([0.8, 0.4, 0.2]) {
            assert!((got - want).abs() < 1e-6, "{c:?}");
        }
        // Nothing there: no colour either, rather than a division by zero.
        assert_eq!(unpremultiply([0.0; 3], 0.0), ([0.0; 3], 0.0));
        assert_eq!(unpremultiply([f32::NAN; 3], f32::NAN), ([0.0; 3], 0.0));
        // A highlight brighter than its coverage raises the alpha to hold
        // it, and the product is still what went in.
        let (c, a) = unpremultiply([0.6, 0.3, 0.3], 0.1);
        assert!((a - 0.6).abs() < 1e-6);
        assert!((c[0] * a - 0.6).abs() < 1e-6 && (c[1] * a - 0.3).abs() < 1e-6);
        assert!(c.iter().all(|v| *v <= 1.0));
    }

    #[test]
    fn resolve_averages_premultiplied_and_stores_the_surface_colour() {
        // Four samples of one pixel: two on a mid-grey surface, two on
        // nothing. Half covered, and the stored colour is the surface's,
        // not the half-dark average.
        let surface = 0.5f32;
        let premult = [[2.0 * surface; 3], [0.0; 3], [4.0 * surface; 3]];
        let alpha = [2.0f32, 0.0, 4.0];
        let img = resolve(&premult, &alpha, 4, 3, 1);
        assert_eq!(img.rgba[3], 128);
        assert!((img.rgba[0] as i32 - to_srgb8(surface) as i32).abs() <= 1, "{:?}", &img.rgba[..4]);
        assert_eq!(&img.rgba[4..8], &[0, 0, 0, 0], "an uncovered pixel is fully transparent black");
        assert_eq!(img.rgba[11], 255);
        assert!((img.rgba[8] as i32 - to_srgb8(surface) as i32).abs() <= 1);
    }

    #[test]
    fn downsample_ignores_the_colour_of_transparent_pixels() {
        // 2×2: one opaque red pixel, three transparent ones that happen to
        // carry green. A quarter covered, and still red.
        let img = Image { width: 2, height: 2, rgba: vec![255, 0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0] };
        let d = downsample(&img, 2);
        assert_eq!((d.width, d.height), (1, 1));
        assert_eq!(d.rgba, vec![255, 0, 0, 64]);
        assert_eq!(downsample(&img, 1).rgba, img.rgba);
    }

    #[test]
    fn over_blends_in_linear_light() {
        let img = Image { width: 2, height: 1, rgba: vec![255, 255, 255, 128, 10, 20, 30, 0] };
        let o = over(&img, [0.0, 0.0, 0.0]);
        // Half of white over black is 0.5 linear, 188 encoded.
        assert!((o.rgba[0] as i32 - 188).abs() <= 1, "{:?}", o.rgba);
        assert_eq!(&o.rgba[4..8], &[0, 0, 0, 255]);
        let o = over(&img, [1.0, 1.0, 1.0]);
        assert_eq!(&o.rgba[..8], &[255, 255, 255, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn the_difference_matte_recovers_colour_and_coverage() {
        // A surface of 0.4 at 0, 30, 70 and 100 % coverage, as a renderer
        // would write it over black and over white.
        let cov = [0.0f32, 0.3, 0.7, 1.0];
        let px = |bg: f32| -> Vec<u8> {
            cov.iter().flat_map(|a| { let v = to_srgb8(0.4 * a + (1.0 - a) * bg); [v, v, v, 255] }).collect()
        };
        let (b, w) = (Image { width: 4, height: 1, rgba: px(0.0) }, Image { width: 4, height: 1, rgba: px(1.0) });
        let m = difference_matte(&b, &w);
        for (i, a) in cov.iter().enumerate() {
            let got = &m.rgba[i * 4..i * 4 + 4];
            assert!((got[3] as f32 / 255.0 - a).abs() < 0.02, "alpha {i}: {got:?}");
            if *a > 0.0 {
                assert!((got[0] as i32 - to_srgb8(0.4) as i32).abs() <= 3, "colour {i}: {got:?}");
            }
        }
        assert_eq!(&m.rgba[..4], &[0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------
    // Whole renders, both CPU backends.

    use super::super::testing::{box_mesh, quad};
    use super::super::{Backend, Camera, Item, RenderOptions, render_with_stats, trace::PathTraceOptions};
    use crate::context::Material;
    use crate::geom::Vec3;

    const BACKENDS: [Backend; 2] = [Backend::Raster, Backend::Path];
    /// Black, white and a saturated teal, display-linear.
    const BACKGROUNDS: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 0.5, 0.5]];
    const W: u32 = 48;

    /// A sheet seen from straight above whose right edge leans a third of a
    /// pixel per row, so the edge crosses every sub-pixel position.
    fn sheet() -> (crate::tessellate::Mesh, Camera) {
        let mesh = quad([-1.0, -1.2, 0.0], [0.4, -1.2, 0.0], [0.8, 1.2, 0.0], [-1.0, 1.2, 0.0]);
        let cam = Camera { eye: Vec3::new(0.0, 0.0, 10.0), target: Vec3::ZERO, up: Vec3::Y, fov_degrees: 20.0, orthographic: true, shift: 0.0 };
        (mesh, cam)
    }

    fn opts(backend: Backend) -> RenderOptions {
        RenderOptions {
            width: W,
            height: W,
            samples: if backend == Backend::Path { 64 } else { 8 },
            backend,
            transparent: true,
            draw_edges: false,
            draw_grid: false,
            path: PathTraceOptions { ground: false, ..PathTraceOptions::default() },
            ..Default::default()
        }
    }

    fn shot(mesh: &crate::tessellate::Mesh, mat: &Material, cam: &Camera, o: &RenderOptions) -> Image {
        render_with_stats(&[Item::new(mesh, mat, &[])], cam, o).0
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * img.width + x) * 4) as usize;
        [img.rgba[o], img.rgba[o + 1], img.rgba[o + 2], img.rgba[o + 3]]
    }

    fn orange() -> Material {
        Material { base_color: [0.9, 0.45, 0.1], roughness: 0.8, ..Material::default() }
    }

    /// Alpha is 0 off the sheet, 1 on it, and across the leaning edge every
    /// row steps down through fractional values without ever rising.
    #[test]
    fn alpha_is_coverage_and_ramps_across_a_straight_edge() {
        let (mesh, cam) = sheet();
        for backend in BACKENDS {
            let img = shot(&mesh, &orange(), &cam, &opts(backend));
            assert_eq!((img.width, img.height), (W, W));
            let mut levels = std::collections::BTreeSet::new();
            for y in 8..W - 8 {
                assert_eq!(px(&img, W - 2, y), [0, 0, 0, 0], "{backend:?}: far outside, row {y}");
                assert_eq!(px(&img, W / 2 - 2, y)[3], 255, "{backend:?}: well inside, row {y}");
                let row: Vec<u8> = (W / 2 - 2..W - 2).map(|x| px(&img, x, y)[3]).collect();
                assert!(row.windows(2).all(|p| p[1] <= p[0]), "{backend:?}: row {y} is not monotone: {row:?}");
                let partial: Vec<u8> = row.iter().copied().filter(|a| *a > 0 && *a < 255).collect();
                assert!(partial.len() <= 2, "{backend:?}: row {y} has a soft edge wider than the pixel filter: {row:?}");
                levels.extend(partial);
            }
            // Eight fixed positions per pixel in the rasterizer, 64 paths here.
            let want = if backend == Backend::Path { 12 } else { 4 };
            assert!(levels.len() >= want, "{backend:?}: the edge is a mask, not coverage: {levels:?}");
        }
    }

    /// Over black, white and teal, every edge pixel lies between the
    /// background and the sheet's own colour, channel by channel: no dark
    /// rim from a background baked into the colour, no light one either.
    #[test]
    fn an_edge_composites_over_any_colour_without_a_fringe() {
        let (mesh, cam) = sheet();
        for backend in BACKENDS {
            let img = shot(&mesh, &orange(), &cam, &opts(backend));
            let (mut lo, mut hi) = ([255u8; 3], [0u8; 3]);
            for y in 8..W - 8 {
                for x in 14..W / 2 {
                    let p = px(&img, x, y);
                    assert_eq!(p[3], 255);
                    for c in 0..3 {
                        lo[c] = lo[c].min(p[c]);
                        hi[c] = hi[c].max(p[c]);
                    }
                }
            }
            const TOL: i32 = 10;
            let mut edges = 0;
            for bg in BACKGROUNDS {
                let comp = over(&img, bg);
                for y in 8..W - 8 {
                    for x in W / 2..W - 2 {
                        let (p, c) = (px(&img, x, y), px(&comp, x, y));
                        if p[3] == 0 || p[3] == 255 {
                            continue;
                        }
                        edges += 1;
                        for k in 0..3 {
                            // The stored colour is the sheet's.
                            assert!(
                                p[k] as i32 >= lo[k] as i32 - TOL && p[k] as i32 <= hi[k] as i32 + TOL,
                                "{backend:?}: edge pixel ({x},{y}) stores {p:?}, sheet is {lo:?}..{hi:?}"
                            );
                            let b = to_srgb8(bg[k]);
                            let (min, max) = (b.min(lo[k]) as i32 - TOL, b.max(hi[k]) as i32 + TOL);
                            assert!(
                                (min..=max).contains(&(c[k] as i32)),
                                "{backend:?}: over {bg:?}, edge pixel ({x},{y}) is {c:?}, outside {min}..{max} in channel {k}"
                            );
                        }
                    }
                }
            }
            assert!(edges > 60, "{backend:?}: only {edges} edge pixels");
        }
    }

    /// The transparent render composited over a colour is the opaque render
    /// on that colour: the alpha is the same coverage the opaque image was
    /// blended with, not a second opinion.
    #[test]
    fn transparent_over_a_colour_matches_the_opaque_render_on_it() {
        let (mesh, cam) = sheet();
        let mat = Material { base_color: [0.95, 0.95, 0.95], ..Material::default() };
        for backend in BACKENDS {
            let clear = shot(&mesh, &mat, &cam, &opts(backend));
            for bg in BACKGROUNDS {
                // The sheet lies on the ground, and the opaque image would
                // put its contact shadow on the backdrop.
                let solid = shot(&mesh, &mat, &cam, &RenderOptions { transparent: false, background: bg, contact_shadow: false, ..opts(backend) });
                let comp = over(&clear, bg);
                let worst = comp.rgba.iter().zip(&solid.rgba).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
                assert!(worst <= 4, "{backend:?} over {bg:?}: off by {worst} levels");
            }
        }
    }

    /// A bright surface against a dark backdrop in an *opaque* path-traced
    /// image: the edge pixels take the values between, in proportion to
    /// coverage. Filtering radiance before the tone curve made each of them
    /// as bright as the surface.
    #[test]
    fn a_bright_edge_on_a_dark_backdrop_is_a_ramp_not_a_staircase() {
        let (mesh, cam) = sheet();
        let mat = Material { base_color: [1.0, 1.0, 1.0], emissive: [6.0, 6.0, 6.0], ..Material::default() };
        let o = RenderOptions { transparent: false, background: [0.0, 0.0, 0.0], tonemap: super::super::ToneMap::Neutral, ..opts(Backend::Path) };
        let img = shot(&mesh, &mat, &cam, &o);
        let clear = shot(&mesh, &mat, &cam, &RenderOptions { transparent: true, ..o.clone() });
        let inside = from_srgb8(px(&img, W / 2 - 2, W / 2)[0]);
        assert!(inside > 0.8, "the surface should be near white, is {inside}");
        let mut checked = 0;
        for y in 8..W - 8 {
            for x in W / 2..W - 2 {
                let a = px(&clear, x, y)[3] as f32 / 255.0;
                if a < 0.15 || a > 0.85 {
                    continue;
                }
                let v = from_srgb8(px(&img, x, y)[0]);
                assert!((v - a * inside).abs() < 0.06, "({x},{y}): coverage {a}, value {v}, surface {inside}");
                checked += 1;
            }
        }
        assert!(checked > 20, "only {checked} edge pixels");
    }

    /// With the shadow catcher, the ground under and beside a block is black
    /// at some alpha, and nothing else of the ground is there; without it
    /// the same pixels are empty.
    #[test]
    fn the_shadow_catcher_writes_the_ground_shadow_as_alpha_alone() {
        let mesh = box_mesh([-1.0, -1.0, 0.0], [1.0, 1.0, 1.0]);
        let cam = Camera { eye: Vec3::new(0.0, 0.0, 12.0), target: Vec3::ZERO, up: Vec3::Y, fov_degrees: 30.0, orthographic: true, shift: 0.0 };
        let mat = Material { base_color: [0.8, 0.8, 0.8], ..Material::default() };
        for backend in BACKENDS {
            let base = RenderOptions { path: PathTraceOptions::default(), ..opts(backend) };
            let bare = shot(&mesh, &mat, &cam, &base);
            let caught = shot(&mesh, &mat, &cam, &RenderOptions { shadow_catcher: true, ..base.clone() });
            let (mut shadowed, mut far) = (0u32, 0u32);
            for y in 0..W {
                for x in 0..W {
                    let (b, c) = (px(&bare, x, y), px(&caught, x, y));
                    if b[3] == 255 {
                        assert_eq!(c[3], 255, "{backend:?}: the block stays opaque at ({x},{y})");
                    }
                    if b[3] != 0 {
                        continue;
                    }
                    if c[3] > 0 {
                        assert!(c[0] < 8 && c[1] < 8 && c[2] < 8, "{backend:?}: a shadow is black, got {c:?} at ({x},{y})");
                        assert!(c[3] < 250, "{backend:?}: a shadow is not a floor, got {c:?} at ({x},{y})");
                        shadowed += 1;
                    }
                    if x < 2 || y < 2 || x >= W - 2 || y >= W - 2 {
                        far = far.max(c[3] as u32);
                    }
                }
            }
            assert!(shadowed > 40, "{backend:?}: only {shadowed} shadow pixels");
            assert!(far < 40, "{backend:?}: the shadow should have faded at the frame edge, alpha {far}");
        }
    }

    /// Clear glass and a translucent paint are partly there: fractional
    /// alpha across the pane, not a hole and not a solid.
    #[test]
    fn glass_is_partly_transparent() {
        let (mesh, cam) = sheet();
        let glass = Material { base_color: [1.0, 1.0, 1.0], roughness: 0.0, transmission: 1.0, ..Material::default() };
        let film = Material { base_color: [0.3, 0.6, 0.9], opacity: 0.4, ..Material::default() };
        for (backend, mat) in [(Backend::Path, &glass), (Backend::Raster, &film), (Backend::Path, &film)] {
            let img = shot(&mesh, mat, &cam, &opts(backend));
            for y in 10..W - 10 {
                let a = px(&img, W / 2 - 4, y)[3];
                assert!(a > 5 && a < 220, "{backend:?} {}: {:?} at row {y}", mat.transmission, px(&img, W / 2 - 4, y));
            }
            assert_eq!(px(&img, W - 2, W / 2)[3], 0);
        }
    }

    /// The ground grid on a transparent render is ink at its own alpha, and
    /// that alpha falls off with distance instead of ending at a rim.
    #[test]
    fn the_grid_carries_its_own_alpha() {
        let mesh = box_mesh([-0.5, -0.5, 0.0], [0.5, 0.5, 1.0]);
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 2.0);
        let mat = Material::default();
        let o = RenderOptions { width: 96, height: 96, draw_grid: true, ..opts(Backend::Raster) };
        let img = shot(&mesh, &mat, &cam, &o);
        let off = shot(&mesh, &mat, &cam, &RenderOptions { draw_grid: false, ..o.clone() });
        let (mut ink, mut solid) = (0u32, 0u32);
        for (g, m) in img.rgba.chunks_exact(4).zip(off.rgba.chunks_exact(4)) {
            if m[3] == 0 && g[3] > 0 {
                ink += 1;
                solid += (g[3] == 255) as u32;
            }
        }
        assert!(ink > 100, "the grid drew {ink} pixels");
        assert!(solid * 4 < ink, "grid lines are soft: {solid} of {ink} pixels are opaque");
    }

    /// Supersampling renders more pixels and filters them down: the size is
    /// the one asked for and a one-sample edge gains the levels between.
    #[test]
    fn supersampling_filters_down_to_the_requested_size() {
        let (mesh, cam) = sheet();
        let one = RenderOptions { samples: 1, ..opts(Backend::Raster) };
        let levels = |img: &Image| img.rgba.chunks_exact(4).map(|p| p[3]).collect::<std::collections::BTreeSet<u8>>().len();
        let plain = shot(&mesh, &orange(), &cam, &one);
        let fine = shot(&mesh, &orange(), &cam, &RenderOptions { supersample: 3, ..one.clone() });
        assert_eq!((fine.width, fine.height), (W, W));
        assert_eq!(levels(&plain), 2, "one sample per pixel is a mask");
        assert!(levels(&fine) >= 6, "3x3 supersampling gives {} alpha levels", levels(&fine));
        // And an opaque render comes back opaque.
        let solid = shot(&mesh, &orange(), &cam, &RenderOptions { supersample: 2, transparent: false, ..one });
        assert!(solid.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }
}
