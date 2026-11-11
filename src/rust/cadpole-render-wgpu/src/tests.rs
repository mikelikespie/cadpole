//! What the GPU renderer is checked against.
//!
//! Every test that needs a device skips, with a printed reason, when there is
//! no adapter: a contributor without a driver and a remote-execution sandbox
//! both hit that path, and neither should see a red test. The WGSL validation
//! test needs no adapter at all, so the shaders stay checked everywhere.

use super::testing::*;
use super::*;
use cadpole_core::context::Material;
use cadpole_core::geom::Vec3;
use cadpole_core::render::{self, EdgeMode, Item};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// One device for the whole test binary: opening even a software adapter costs
/// tens of milliseconds, and the tests run in parallel threads.
fn gpu() -> Option<MutexGuard<'static, Renderer>> {
    static GPU: OnceLock<Option<Mutex<Renderer>>> = OnceLock::new();
    let slot = GPU.get_or_init(|| match Renderer::new(RendererOptions::new()) {
        Ok(r) => {
            println!("cadpole-render-wgpu tests on adapter: {}", r.adapter());
            Some(Mutex::new(r))
        }
        Err(e) => {
            println!("SKIPPING every GPU test: {e}");
            None
        }
    });
    slot.as_ref().map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

fn scene() -> (cadpole_core::tessellate::Mesh, Vec<Vec<Vec3>>, Vec<f64>, Camera) {
    let (mesh, edges, dihedrals) = boxy(Vec3::new(-10.0, -6.0, 0.0), Vec3::new(10.0, 6.0, 14.0));
    let (lo, hi) = mesh.bbox().unwrap();
    let cam = Camera::fitted("iso", lo, hi, 0.15, 320.0 / 240.0);
    (mesh, edges, dihedrals, cam)
}

fn opts(width: u32, height: u32) -> RenderOptions {
    RenderOptions { width, height, ..Default::default() }
}

fn to_srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// Every WGSL module parses and passes naga's validator. This is the one test
/// that needs no GPU at all, so the shaders stay checked on a machine with no
/// driver -- which is also why `naga` is a direct dependency.
#[test]
fn wgsl_modules_are_valid() {
    let both = pipelines::modules(true).into_iter().chain(pipelines::modules(false));
    for (name, source) in both {
        let module = match naga::front::wgsl::parse_str(&source) {
            Ok(m) => m,
            Err(e) => panic!("{name}.wgsl does not parse:\n{}", e.emit_to_string(&source)),
        };
        let mut validator =
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty());
        if let Err(e) = validator.validate(&module) {
            panic!("{name}.wgsl does not validate:\n{}", e.emit_to_string(&source));
        }
    }
}

/// Opening a renderer either works or says why; it never panics. On a machine
/// with no driver this is the whole of the GPU story, and
/// `cadpole render --backend gpu` falls back on exactly this error.
#[test]
fn missing_adapter_is_reported_not_panicked() {
    match Renderer::new(RendererOptions::new()) {
        Ok(r) => assert!(!r.adapter().is_empty()),
        Err(Error::NoAdapter(m)) => assert!(!m.is_empty(), "the diagnostic must say something"),
        Err(e) => panic!("unexpected error opening a renderer: {e}"),
    }
}

/// Renderers opened from several threads at once, while others are in use:
/// a server rendering requests in parallel, or this test binary, whose
/// tests run on parallel threads. Each open used to create its own Vulkan
/// instance, and the loader unloads unused drivers (`dlclose`) while one
/// instance enumerates devices, under another thread creating its device:
/// an intermittent segfault, never a test failure with a name. One instance
/// per process, and opens taken one at a time, make that impossible.
#[test]
fn renderers_open_concurrently() {
    let Some(_shared) = gpu() else { return };
    drop(_shared);
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..4 {
                    match Renderer::new(RendererOptions::new()) {
                        Ok(r) => assert!(!r.adapter().is_empty()),
                        Err(e) => panic!("a second renderer failed to open: {e}"),
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("an opening thread panicked");
    }
}

/// The headline test: a GPU render of a box is within a bounded perceptual
/// difference of the CPU render of the same scene.
///
/// Metric: absolute difference per sRGB8 channel (`diff::compare`), with the
/// mean, the 99th percentile and the share of pixels whose coverage disagrees
/// all bounded. The default options are the hard case -- they include the two
/// screen-space stages (SSAO, contact shadow) and the whole line overlay.
#[test]
fn gpu_matches_the_cpu_on_a_box() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    let o = opts(320, 240);
    let cpu = render::render(&items, &cam, &o);
    let gpu = r.render(&items, &cam, &o).expect("gpu render");
    let d = diff::compare(&cpu, &gpu, o.background.map(to_srgb8));
    println!("box, default options: {d}");
    // Measured on lavapipe: mean 0.084/255, p99 1/255, 0.32% of pixels with
    // different coverage (the silhouette, one pixel wide). The bounds are an
    // order of magnitude above that, which leaves room for another driver's
    // multisample positions without leaving room for a wrong pixel.
    assert!(d.mean < 1.0, "mean difference too large: {d}");
    assert!(d.p99 <= 4, "tail too heavy: {d}");
    assert!(d.coverage < 0.015, "silhouettes disagree: {d}");
}

/// The same scene with the screen-space stages the two renderers implement
/// differently switched off, which isolates the shading model: same GGX, same
/// three-light rig, same gradient environment, same ACES tonemap, same sRGB
/// resolve.
#[test]
fn shading_alone_matches_the_cpu_closely() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material { base_color: [0.82, 0.55, 0.42], roughness: 0.45, ..Material::default() };
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    let o = RenderOptions {
        width: 320,
        height: 240,
        draw_grid: false,
        draw_edges: false,
        ambient_occlusion: false,
        contact_shadow: false,
        samples: 1,
        ..Default::default()
    };
    let cpu = render::render(&items, &cam, &o);
    let gpu = r.render(&items, &cam, &o).expect("gpu render");
    let d = diff::compare(&cpu, &gpu, o.background.map(to_srgb8));
    println!("box, shading only: {d}");
    // Measured on lavapipe: mean 0.008/255, max 1/255 -- the two are the same
    // arithmetic to within the last bit of an 8-bit channel.
    assert!(d.mean < 0.2, "shading model differs: {d}");
    assert!(d.p99 <= 2, "shading model differs in the tail: {d}");
}

/// The WGSL port of `render/textures.rs` gives the same pixels as the Rust
/// one. Both sample the same procedural function at the same object-space
/// positions with the same footprint, so this is a tight bound: anything that
/// drifts (a hash, a fade curve, a pattern mean) shows up here long before it
/// shows up in a screenshot. The two machined finishes (`turned`, `milled`)
/// are relief kinds the shaders do not carry — they tilt the normal from a
/// per-face frame the viewport has no buffer for — so they are not compared.
#[test]
fn textures_match_the_cpu_port() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    for texture in render::texture_names().iter().filter(|t| !render::texture_kind(t).is_some_and(|k| k.is_relief())) {
        let mat = material(texture, 6.0);
        let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
        let o = RenderOptions {
            width: 256,
            height: 192,
            draw_grid: false,
            draw_edges: false,
            ambient_occlusion: false,
            contact_shadow: false,
            samples: 1,
            ..Default::default()
        };
        let cpu = render::render(&items, &cam, &o);
        let gpu = r.render(&items, &cam, &o).expect("gpu render");
        let d = diff::compare(&cpu, &gpu, o.background.map(to_srgb8));
        println!("texture {texture}: {d}");
        // Measured on lavapipe: mean 0.009..0.023/255 and max 1/255 for all
        // five patterns.
        assert!(d.mean < 0.5, "texture {texture} differs: {d}");
        assert!(d.over_8 < 0.005, "texture {texture} differs on too many pixels: {d}");
    }
}

/// The id buffer names the right face at a known pixel: the top of the box is
/// face 5, and the centre of a top view lands on it. The point of the pass is
/// that this costs no raycast and no BVH.
#[test]
fn picking_returns_the_face_under_a_known_pixel() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, _) = scene();
    let (lo, hi) = mesh.bbox().unwrap();
    let cam = Camera::fitted("top", lo, hi, 0.15, 320.0 / 240.0);
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    let o = opts(320, 240);
    r.render(&items, &cam, &o).expect("gpu render");
    let hit = r.pick(160, 120, 0).expect("something under the centre");
    assert_eq!(hit.item, 0);
    assert_eq!(hit.face, Some(5), "the +z face of the box");
    assert_eq!(hit.edge, None);
    // A pixel in the corner of the frame is empty at radius 0.
    assert_eq!(r.pick(2, 2, 0), None);
    // And an edge is pickable somewhere along the middle row, winning over the
    // face behind it.
    let mut edge_hits = 0;
    for x in 0..320 {
        if let Some(p) = r.pick(x, 120, 2) {
            edge_hits += usize::from(p.edge.is_some());
        }
    }
    assert!(edge_hits > 0, "no edge was pickable anywhere along the middle row");
}

/// Two renders of the same scene on the same device are byte-identical.
/// Determinism *across* drivers is not claimed and cannot be; that is what the
/// CPU rasterizer is for.
#[test]
fn renders_are_deterministic() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    let o = opts(200, 150);
    let a = r.render(&items, &cam, &o).expect("gpu render");
    let b = r.render(&items, &cam, &o).expect("gpu render");
    assert_eq!(a.rgba, b.rgba, "the same scene rendered twice differs");
    // And the corner is exactly the background, as on the CPU.
    assert_eq!(&a.rgba[0..3], &o.background.map(to_srgb8));
}

/// Sample counts snap the way the CPU renderer's do, then step down to what
/// the device accepts for every G-buffer format (the adapter's own counts
/// when the device took TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES, else
/// WebGPU's guaranteed 1 and 4), and every one of them renders with the
/// background bit-exact.
#[test]
fn sample_counts_follow_the_cpu_rules() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];

    // The device took the feature exactly when the adapter offered it.
    let tasff = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    assert_eq!(r.gpu.device.features().contains(tasff), r.gpu.adapter.features().contains(tasff));
    // What the device accepts for 8 samples, by the rule wgpu validates with.
    let eight_ok = [pipelines::NORMAL, pipelines::IDS, pipelines::DEPTH].iter().all(|f| {
        let flags = if r.gpu.device.features().contains(tasff) {
            r.gpu.adapter.get_texture_format_features(*f).flags
        } else {
            f.guaranteed_format_features(r.gpu.device.features()).flags
        };
        flags.sample_count_supported(8)
    });

    for (samples, expect) in [(1u32, 1u32), (4, 4), (8, if eight_ok { 8 } else { 4 })] {
        let o = RenderOptions { samples, ..opts(160, 120) };
        assert_eq!(r.sample_count(&o), expect, "samples={samples}");
        let img = r.render(&items, &cam, &o).expect("gpu render");
        assert_eq!(&img.rgba[0..3], &o.background.map(to_srgb8), "samples={samples}");
    }
}

/// Opacity reaches the GPU: a translucent body does not look like a solid one.
#[test]
fn opacity_is_honoured() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let solid = Material::default();
    let glass = Material { opacity: 0.25, ..Material::default() };
    let o = RenderOptions { draw_grid: false, draw_edges: false, ..opts(160, 120) };
    let a = r
        .render(&[Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &solid, &edges) }], &cam, &o)
        .expect("gpu render");
    let b = r
        .render(&[Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &glass, &edges) }], &cam, &o)
        .expect("gpu render");
    let d = diff::compare(&a, &b, o.background.map(to_srgb8));
    println!("opaque vs 25% opacity: {d}");
    assert!(d.mean > 2.0, "a translucent body should not look like a solid one: {d}");
}

/// Guide geometry casts no contact shadow on the GPU either: the
/// heightfield it bakes on the CPU leaves a `shadow: false` body out, as
/// `occlusion::Heightfield` does, so a guide box leaves every cell of the
/// baked map at exactly 1 where the same box casting shadows darkens the
/// ground under it. Needs no adapter: the map is built before anything is
/// uploaded.
#[test]
fn a_guide_body_casts_no_contact_shadow() {
    let (mesh, edges, dihedrals, cam) = scene();
    let solid = Material::default();
    let guide = Material { shadow: false, ..Material::default() };
    let o = RenderOptions { contact_shadow: true, ..opts(160, 120) };
    let bake = |mat: &Material| {
        let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, mat, &edges) }];
        crate::scene::Upload::build(&items, &cam, &o).heightfield
    };
    let (cast, guided) = (bake(&solid), bake(&guide));
    assert!(cast.enabled && guided.enabled, "the contact shadow is on for both");
    let darkest = cast.shadow.iter().copied().fold(1.0f32, f32::min);
    assert!(darkest < 0.5, "a solid box darkens the ground under it: {darkest}");
    assert!(guided.shadow.iter().all(|&s| s == 1.0), "a guide leaves the ground untouched");
}

/// Orthographic projection reaches the same pixels the CPU's does. The depth
/// encoding differs between the two projections (reverse-Z against a linear
/// ramp), so this is the test that catches a wrong matrix.
#[test]
fn orthographic_matches_the_cpu() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, mut cam) = scene();
    cam.orthographic = true;
    let (lo, hi) = mesh.bbox().unwrap();
    cam.fit(lo, hi, 0.15, 320.0 / 240.0);
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    let o = opts(320, 240);
    let cpu = render::render(&items, &cam, &o);
    let gpu = r.render(&items, &cam, &o).expect("gpu render");
    let d = diff::compare(&cpu, &gpu, o.background.map(to_srgb8));
    println!("orthographic: {d}");
    // Measured on lavapipe: mean 0.059/255, 0.18% coverage disagreement.
    assert!(d.coverage < 0.015, "orthographic silhouettes disagree: {d}");
    assert!(d.mean < 1.0, "orthographic shading disagrees: {d}");
}

/// The overlay and the selection highlight both reach the GPU: turning edges
/// off changes the image, and highlighting a face tints it.
#[test]
fn overlays_and_highlights_change_the_image() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let o = opts(240, 180);
    let with_edges = r
        .render(&[Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }], &cam, &o)
        .expect("gpu render");
    let plain = r
        .render(
            &[Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }],
            &cam,
            &RenderOptions { edges: EdgeMode::Off, draw_silhouettes: false, ..o.clone() },
        )
        .expect("gpu render");
    let d = diff::compare(&with_edges, &plain, o.background.map(to_srgb8));
    println!("edges on vs off: {d}");
    assert!(d.over_8 > 0.002, "the edge overlay drew nothing: {d}");

    let faces = [5u32];
    let hl = Item { edge_dihedrals: &dihedrals, highlight_faces: &faces, ..Item::new(&mesh, &mat, &edges) };
    let selected = r.render(&[hl], &cam, &o).expect("gpu render");
    let d = diff::compare(&with_edges, &selected, o.background.map(to_srgb8));
    println!("selection highlight: {d}");
    assert!(d.over_8 > 0.02, "the selection highlight did not tint the face: {d}");
}

/// What the GPU costs against the CPU rasterizer, printed rather than
/// asserted: a threshold here would be a flake on a shared machine, but the
/// number is the reason the viewport is moving to the GPU at all.
#[test]
fn reports_timings_against_the_cpu() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    for (w, h) in [(320u32, 240u32), (1280, 960)] {
        let o = opts(w, h);
        // Warm the pipelines and the targets, which a viewport pays once.
        let _ = r.render(&items, &cam, &o).expect("gpu render");
        let n = 3;
        let t0 = now_ms();
        for _ in 0..n {
            let _ = r.render(&items, &cam, &o).expect("gpu render");
        }
        let gpu_ms = (now_ms() - t0) / f64::from(n);
        let t0 = now_ms();
        for _ in 0..n {
            let _ = render::render(&items, &cam, &o);
        }
        let cpu_ms = (now_ms() - t0) / f64::from(n);
        println!("{w}x{h}: gpu {gpu_ms:.1} ms (incl. readback), cpu {cpu_ms:.1} ms, on {}", r.adapter());
    }
}

/// A surface cannot be created headlessly, but the paths that do not need one
/// are still checkable: the canvas arm reports why it is not wired, rather
/// than silently doing something else.
#[test]
fn canvas_surface_is_refused_with_a_reason() {
    let Some(r) = gpu() else { return };
    let message = match unsafe { Viewport::new(&r, SurfaceSource::Canvas, 64, 64) } {
        Ok(_) => panic!("there is no canvas in a headless test"),
        Err(e) => e.to_string(),
    };
    assert!(message.contains("wasm-bindgen"), "the diagnostic should name the blocker: {message}");
}

/// The render-on-demand state machine, which is the part of the surface path
/// that has logic rather than a platform handle in it: a fresh viewport owes a
/// frame, drawing settles it, a suspended one never draws however dirty it is,
/// and coming back from the background owes one frame.
#[test]
fn render_on_demand_settles_and_survives_backgrounding() {
    let mut d = Demand::new();
    assert!(d.needs_render(), "a fresh viewport owes a frame");
    d.drawn();
    assert!(!d.needs_render(), "a quiescent viewport must let the display link pause");
    d.set_needs_render();
    assert!(d.needs_render());
    d.suspend();
    assert!(!d.needs_render(), "a backgrounded viewport never draws");
    assert!(d.is_suspended());
    d.drawn();
    d.resume();
    assert!(d.needs_render(), "coming back to the foreground owes a frame");
}

/// The presentation pipeline compiles for the swapchain formats a platform is
/// likely to hand back, which is as much of the surface path as a machine with
/// no window can check.
#[test]
fn present_pipelines_compile_for_swapchain_formats() {
    let Some(mut r) = gpu() else { return };
    let (mesh, edges, dihedrals, cam) = scene();
    let mat = Material::default();
    let items = [Item { edge_dihedrals: &dihedrals, ..Item::new(&mesh, &mat, &edges) }];
    // The blit pipeline is built against the renderer's own layouts, so one
    // frame has to have been rendered first.
    r.render_to_texture(&items, &cam, &opts(64, 48)).expect("gpu render");
    for format in [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm] {
        surface::blit_pipeline(&r, format).unwrap_or_else(|e| panic!("no blit pipeline for {format:?}: {e}"));
    }
}

/// The diff helper itself: identical images are identical, and a one-step
/// difference is reported as one step.
#[test]
fn diff_helper_measures_what_it_says() {
    let a = Image::new(4, 4, [0.5, 0.5, 0.5]);
    let mut b = a.clone();
    assert_eq!(diff::compare(&a, &b, [0; 3]).mean, 0.0);
    b.rgba[0] = b.rgba[0].wrapping_add(3);
    let d = diff::compare(&a, &b, [0; 3]);
    assert_eq!(d.max, 3);
    assert!((d.mean - 3.0 / 48.0).abs() < 1e-9);
}
