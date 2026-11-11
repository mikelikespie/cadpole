//! Render-path regressions: what reaches the image from the scene builder
//! (`scene_from_context`) and the request options, checked on the pixels
//! the `Render` rpc returns.

use cadpole_core::render::Image;
use cadpole_proto::v1 as pb;

use crate::{document_from_scripts, render};

fn doc(src: &str) -> pb::Document {
    document_from_scripts("t", vec![("main.star".into(), src.into())])
}

fn request(src: &str, view: &str) -> pb::RenderRequest {
    pb::RenderRequest {
        document: Some(doc(src)),
        width: 200,
        height: 150,
        camera: Some(pb::Camera { preset: view.into(), ..Default::default() }),
        ..Default::default()
    }
}

fn image(req: &pb::RenderRequest) -> Image {
    let r = render(req).unwrap();
    Image::from_png(&r.png).unwrap()
}

fn count(img: &Image, f: impl Fn([u8; 3]) -> bool) -> usize {
    img.rgba.chunks_exact(4).filter(|p| f([p[0], p[1], p[2]])).count()
}

fn red(p: [u8; 3]) -> bool {
    p[0] > 130 && p[1] < 90 && p[2] < 90
}

fn centre(img: &Image) -> [u8; 3] {
    let i = (((img.height / 2) * img.width + img.width / 2) * 4) as usize;
    [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
}

/// `paint(body, faces=...)` colours those faces in every CPU backend, the
/// way `b.material(color=)` colours the whole body; the module doc of
/// `cadpole_core::paint` promises the viewport shows paint as it will print.
#[test]
fn painted_faces_render_in_the_paint_colour() {
    let src = "b = box(40, 40, 20)\npaint(b, faces=[\"box_1/top\"], color=\"#ff0000\")\n";
    for backend in [pb::RenderBackend::Raster, pb::RenderBackend::PathTrace] {
        let mut req = request(src, "top");
        req.set_backend(backend);
        req.samples = 4;
        let img = image(&req);
        assert!(red(centre(&img)), "{backend:?}: the painted top is red, got {:?}", centre(&img));
        assert!(count(&img, red) > 5000, "{backend:?}: {} red pixels", count(&img, red));
    }
    // Seen from the front, the top is a sliver and the sides are not painted.
    let img = image(&request(src, "front"));
    assert!(!red(centre(&img)), "an unpainted side keeps the body colour: {:?}", centre(&img));
}

/// Paint changes a face's colour and nothing else about the body: a metal
/// body painted only on its hidden bottom face renders as it does with no
/// paint at all (it used to turn into matte plastic everywhere), and a body
/// drawn see-through (`--xray`) with a painted face still shows the box
/// inside it (the path tracer drew it opaque).
#[test]
fn paint_keeps_the_bodys_material() {
    let differ = |a: &Image, b: &Image| {
        a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)).filter(|(p, q)| (0..3).any(|k| (p[k] as i32 - q[k] as i32).abs() > 8)).count()
    };
    let metal = "b = box(40, 40, 20)\nb.material(color=\"#c0c0c0\", metallic=1.0, roughness=0.15)\n";
    let nested = "outer = box(40, 40, 40, at=(-20, -20, 0))\ninner = box(10, 10, 10, at=(-5, -5, 15)).material(color=\"#ff0000\")\n";
    for backend in [pb::RenderBackend::Raster, pb::RenderBackend::PathTrace] {
        let run = |src: &str, xray: bool| {
            let mut req = request(src, "iso");
            req.set_backend(backend);
            req.samples = 8;
            req.xray = xray;
            image(&req)
        };
        let plain = run(metal, false);
        let painted = run(&format!("{metal}paint(b, faces=[\"box_1/bottom\"], color=\"#ff0000\")\n"), false);
        let n = differ(&plain, &painted);
        assert!(n < 300, "{backend:?}: painting the hidden bottom changed {n} pixels of a metal body");
        let reddish = |p: [u8; 3]| i32::from(p[0]) - i32::from(p[1]) > 40;
        let plain = run(nested, true);
        let painted = run(&format!("{nested}paint(outer, faces=[\"box_1/bottom\"], color=\"#0000ff\")\n"), true);
        assert!(count(&plain, reddish) > 100, "{backend:?}: the premise, the inner box through xray: {}", count(&plain, reddish));
        assert!(
            count(&painted, reddish) > count(&plain, reddish) / 2,
            "{backend:?}: the painted body went opaque ({} reddish pixels, {} without paint)",
            count(&painted, reddish),
            count(&plain, reddish)
        );
    }
    // And the painted face itself shows the paint on the metal body.
    let img = image(&request(&format!("{metal}paint(b, faces=[\"box_1/top\"], color=\"#ff0000\")\n"), "top"));
    let c = centre(&img);
    assert!(c[0] as i32 > c[1] as i32 + 40 && c[0] as i32 > c[2] as i32 + 40, "the painted top is red: {c:?}");
}

/// A whole-body paint (no `faces=`) recolours the body.
#[test]
fn a_whole_body_paint_renders_in_the_paint_colour() {
    let img = image(&request("b = box(40, 40, 20)\npaint(b, color=\"#ff0000\")\n", "iso"));
    assert!(red(centre(&img)), "{:?}", centre(&img));
}

/// `emboss(kind=emboss.paint, color=)` records a paint feature on the
/// imprinted faces; the disc shows in the render.
#[test]
fn a_painted_emboss_shows_its_colour() {
    let src = "b = box(40, 40, 20)\nart = sketch().circle(8)\nemboss(b, \"box_1/top\", art, kind=emboss.paint, origin=(20, 20, 20), color=\"#ff0000\")\n";
    let img = image(&request(src, "top"));
    assert!(count(&img, red) > 500, "{} red pixels", count(&img, red));
}

/// The section view cuts the mesh; paint on the faces that survive the cut
/// stays where it was.
#[test]
fn paint_survives_a_section_cut() {
    let mut req = request("b = box(40, 40, 20)\npaint(b, faces=[\"box_1/top\"], color=\"#ff0000\")\n", "top");
    req.section = "x=10".into();
    let img = image(&req);
    assert!(count(&img, red) > 1000, "{} red pixels", count(&img, red));
}

/// `xray` draws every body translucent: a box hidden inside another shows
/// through it, in the rasterizer and in the path tracer. It used to be
/// plumbed into the options and never read, so the image was byte for byte
/// the same as without it.
#[test]
fn xray_shows_a_body_inside_another() {
    let src = "outer = box(40, 40, 40, at=(-20, -20, 0))\ninner = box(10, 10, 10, at=(-5, -5, 15)).material(color=\"#ff0000\")\n";
    for backend in [pb::RenderBackend::Raster, pb::RenderBackend::PathTrace] {
        let mut req = request(src, "iso");
        req.set_backend(backend);
        req.samples = 8;
        let plain = image(&req);
        assert_eq!(count(&plain, red), 0, "{backend:?}: the inner box is hidden without xray");
        req.xray = true;
        let xray = image(&req);
        assert_ne!(plain.rgba, xray.rgba, "{backend:?}: xray changes the image");
        let reddish = |p: [u8; 3]| i32::from(p[0]) - i32::from(p[1]) > 40;
        assert!(count(&xray, reddish) > 100, "{backend:?}: the inner box shows through: {}", count(&xray, reddish));
    }
}

/// A free camera (`--eye` without `--target`) looks at the centre of what
/// is drawn. A body on a hidden layer is not drawn, so it does not pull the
/// target away: here it used to move the target 300 mm off and leave the
/// image empty.
#[test]
fn a_free_camera_targets_only_what_is_shown() {
    let src = "far = layer(\"far\", visible=False)\na = box(40, 40, 20).material(color=\"#ff0000\")\nb = box(20, 20, 60, at=(600, 0, -300), layer=far).material(color=\"#00ff00\")\n";
    let eye = |req: &mut pb::RenderRequest| {
        req.camera = Some(pb::Camera {
            eye: Some(pb::Vec3 { x: 200.0, y: -200.0, z: 150.0 }),
            ..Default::default()
        });
    };
    let mut req = request(src, "");
    eye(&mut req);
    let hidden = image(&req);
    assert!(count(&hidden, red) > 500, "the visible box is in frame: {} red pixels", count(&hidden, red));

    // `body_ids` filters the same way.
    let mut req = request("a = box(40, 40, 20).material(color=\"#ff0000\")\nb = box(20, 20, 60, at=(600, 0, -300))\n", "");
    eye(&mut req);
    req.body_ids = vec!["box_1#0".into()];
    let only = image(&req);
    assert!(count(&only, red) > 500, "{} red pixels", count(&only, red));
}

/// Intersection over union of two images' red masks: 1 is the same
/// silhouette, pixel for pixel. A few path-traced samples leave stray red
/// pixels on the floor, so a bounding box would measure the noise.
fn red_overlap(a: &Image, b: &Image) -> f64 {
    let (mut both, mut either) = (0usize, 0usize);
    for (p, q) in a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)) {
        let (x, y) = (red([p[0], p[1], p[2]]), red([q[0], q[1], q[2]]));
        both += usize::from(x && y);
        either += usize::from(x || y);
    }
    if either == 0 { 0.0 } else { both as f64 / either as f64 }
}

/// A model far from the origin (projected site coordinates) or very large
/// renders as it does at the origin. The path tracer formed each primary
/// ray by differencing two f32 world points a millimetre apart, which at
/// 1e7 mm snaps to a handful of directions (a red half-plane), and for a
/// 1e8 mm box, whose eye sits near 1e9, to zero (an empty image).
#[test]
fn the_path_tracer_draws_a_model_far_from_the_origin() {
    let render_path = |src: &str| {
        let mut req = request(src, "iso");
        req.set_backend(pb::RenderBackend::PathTrace);
        req.samples = 4;
        req.width = 100;
        req.height = 75;
        image(&req)
    };
    let near = render_path("a = box(40, 40, 20).material(color=\"#ff0000\")\n");
    assert!(count(&near, red) > 1000, "{}", count(&near, red));
    for (what, src) in [
        ("offset 1e7", "a = box(40, 40, 20, at=(1e7, 1e7, 0)).material(color=\"#ff0000\")\n"),
        ("offset 3e6", "a = box(40, 40, 20, at=(3e6, -3e6, 0)).material(color=\"#ff0000\")\n"),
        // The same box scaled by 1e6: the camera frames it the same.
        ("size 4e7", "a = box(4e7, 4e7, 2e7).material(color=\"#ff0000\")\n"),
    ] {
        let far = render_path(src);
        let iou = red_overlap(&near, &far);
        assert!(iou > 0.85, "{what}: the box is drawn and framed as at the origin: red overlap {iou:.3}, {} red pixels against {}", count(&far, red), count(&near, red));
    }
}

/// The rasterizer's ground grid far from the origin is the grid it draws
/// near it. Its plane hits were world points differenced a pixel apart, and
/// a world point near 1e7 mm is only known to ~1 mm in f32, so the grid came
/// out as horizontal bands. Both boxes sit on a major grid crossing, so the
/// two images should match but for rounding.
#[test]
fn the_raster_grid_far_from_the_origin_matches_the_grid_near_it() {
    let render_raster = |src: &str| {
        let mut req = request(src, "iso");
        req.width = 160;
        req.height = 120;
        image(&req)
    };
    let near = render_raster("a = box(40, 40, 20, at=(1000, 1000, 0)).material(color=\"#ff0000\")\n");
    let far = render_raster("a = box(40, 40, 20, at=(1e7, 1e7, 0)).material(color=\"#ff0000\")\n");
    let differ = near
        .rgba
        .chunks_exact(4)
        .zip(far.rgba.chunks_exact(4))
        .filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 6))
        .count();
    let total = (near.width * near.height) as usize;
    // The grid is faint ink, a few levels over the floor, so the bands are
    // a small difference over many pixels.
    assert!(differ * 50 < total, "{differ} of {total} pixels differ by more than 6 levels");
}
