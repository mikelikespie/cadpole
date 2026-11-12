//! `Animate`: the runner behind `cadpole animate`, the `animate` MCP
//! tool and the route. The declaration and its sampling are
//! `cadpole_core::animation`'s, the encoders `cadpole_core::render::anim`'s; this
//! is the loop that joins them to the evaluator and the renderer.
//!
//! **One memo, one evaluation per frame.** The document is evaluated once
//! per frame with the keyframes' interpolated values as parameter
//! overrides, every evaluation reading and filling one kernel-op memo,
//! exactly as `RunTests` shares one across its sweep points. A
//! frame that only poses instances — a gear turning, a lid lifting — is
//! then Starlark plus rigid bakes: zero kernel ops and zero tessellations
//! after the first frame, which `frame_stats` reports and a test asserts.
//! Nothing is baked *for* the animation: a frame's scene is the part
//! meshes under their instances' transforms, as every render's is.
//!
//! **One camera.** Every frame is drawn with a camera fitted once, to the
//! union of the scene bounding boxes at every keyframe, so nothing jumps
//! or drifts as parts move. A keyframe's `view(...)` turns that camera
//! about the same target, changes its lens, pushes it in (`zoom`) or
//! slides it (`pan`); the one distance is the largest any frame needs for
//! the union box at the image's aspect ratio, so the subject stays in the
//! picture whatever the camera does (`sequence_distance`).
//!
//! **Streaming.** Frames are rendered in order and handed to the APNG
//! encoder as they come (it keeps one previous frame); a GIF needs its
//! palette before its first frame, so its frames are kept as PNG bytes
//! (a few hundred kilobytes each) and decoded twice, once for the palette
//! sample and once to dither. `format = "frames"` keeps the same PNGs and
//! returns them.

use std::collections::HashMap;
use std::sync::Arc;

use cadpole_core::animation::{AnimationDecl, CameraKey, MAX_FRAMES};
use cadpole_core::context::{Diagnostic, Severity};
use cadpole_core::memo::Memo;
use cadpole_core::render::{Camera, Image, RenderOptions, anim};
use cadpole_core::time::Stopwatch;
use cadpole_core::{ParamValue, Vec3};
use cadpole_proto::v1 as pb;

use crate::{
    EvalOptions, Look, RenderScene, apply_document_defaults, camera_from_proto_aspect, document_camera,
    evaluate_document_options, is_free_camera, limits, overrides_from_proto, render_options_from_proto,
    render_scene_stats, scene_from_context, section_scene,
};

/// The formats `AnimateRequest.format` accepts, in documentation order.
pub const FORMATS: [&str; 3] = ["apng", "gif", "frames"];

/// `AnimateRequest` carries `RenderRequest`'s look and quality knobs under
/// the same names; this is the render request they amount to, so the
/// options builder and the clamps are shared rather than copied.
pub fn render_request_of(req: &pb::AnimateRequest) -> pb::RenderRequest {
    pb::RenderRequest {
        document: None,
        width: req.width,
        height: req.height,
        camera: req.camera.clone(),
        backend: req.backend,
        samples: req.samples,
        background: req.background.clone(),
        tolerance: req.tolerance,
        body_ids: req.body_ids.clone(),
        param_overrides: req.param_overrides.clone(),
        threads: req.threads,
        draft: req.draft,
        edge_crease_degrees: req.edge_crease_degrees,
        edge_mode: req.edge_mode,
        grid_mode: req.grid_mode,
        xray: req.xray,
        no_ambient_occlusion: req.no_ambient_occlusion,
        no_contact_shadow: req.no_contact_shadow,
        no_silhouettes: req.no_silhouettes,
        exposure: req.exposure,
        no_edges: req.no_edges,
        no_grid: req.no_grid,
        edge_width_px: req.edge_width_px,
        boolean_tolerance: req.boolean_tolerance,
        max_bounces: req.max_bounces,
        show_environment: req.show_environment,
        lighting: req.lighting.clone(),
        backdrop: req.backdrop.clone(),
        layers: req.layers.clone(),
        floor: req.floor.clone(),
        tonemap: req.tonemap.clone(),
        dof_f_number: req.dof_f_number,
        bloom: req.bloom,
        edge_break_mm: req.edge_break_mm,
        denoise: req.denoise,
        sun: req.sun.clone(),
        north_degrees: req.north_degrees,
        section: req.section.clone(),
        transparent: req.transparent,
        shadow_catcher: req.shadow_catcher,
        supersample: req.supersample,
        sun_color: req.sun_color.clone(),
        sun_intensity: req.sun_intensity,
        haze: req.haze.clone(),
        ..Default::default()
    }
}

/// The azimuth and elevation, in degrees, of a preset's view direction, and
/// its lens: what a camera keyframe interpolates from when the request's
/// `--view` is a preset. (`az=` reads back as itself.)
pub fn preset_key(preset: &str, fov: f64) -> CameraKey {
    let name = if preset.is_empty() { "iso" } else { preset };
    let dir = Camera::preset_direction(name).unwrap_or_else(|| Camera::preset_direction("iso").unwrap());
    let az = dir.x.atan2(-dir.y).to_degrees();
    let el = dir.z.clamp(-1.0, 1.0).asin().to_degrees();
    CameraKey {
        az: Some(az),
        el: Some(el),
        fov: Some(if fov > 0.0 { fov } else { Camera::preset_fov(name) }),
        eye: None,
        target: None,
        zoom: Some(1.0),
        pan: Some((0.0, 0.0)),
    }
}

/// The unit direction from the target toward the eye for an azimuth and an
/// elevation in degrees: `Camera::angular_direction`'s convention.
fn direction(az: f64, el: f64) -> Vec3 {
    let (az, el) = (az.to_radians(), el.to_radians());
    Vec3::new(az.sin() * el.cos(), -az.cos() * el.cos(), el.sin()).normalized()
}

/// A fitted camera turned to `key` about `centre` at distance `dist`,
/// before any zoom or pan: the direction and the lens a frame looks with.
fn turned(base: &Camera, base_key: CameraKey, key: Option<CameraKey>, centre: Vec3, dist: f64) -> Camera {
    let mut cam = base.clone();
    // Nothing to turn and nowhere to move: the base, bit for bit.
    let stands = (base.eye - base.target).length();
    if key.is_none() && centre == base.target && (stands - dist).abs() <= 1e-12 * stands.max(1.0) {
        return cam;
    }
    let mut dir = (base.eye - base.target).normalized();
    if let Some(k) = key {
        let az = k.az.or(base_key.az).unwrap_or(45.0);
        let el = k.el.or(base_key.el).unwrap_or(0.0);
        dir = direction(az, el);
        cam.up = if dir.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
        if let Some(f) = k.fov {
            cam.fov_degrees = f;
        }
    }
    cam.target = centre;
    cam.eye = centre + dir * dist;
    cam
}

fn zoom_of(key: Option<CameraKey>) -> f64 {
    key.and_then(|k| k.zoom).filter(|z| z.is_finite() && *z > 0.0).unwrap_or(1.0)
}

fn pan_of(key: Option<CameraKey>) -> (f64, f64) {
    key.and_then(|k| k.pan).unwrap_or((0.0, 0.0))
}

fn places_eye(key: Option<CameraKey>) -> bool {
    key.map(|k| k.eye.is_some() || k.target.is_some()).unwrap_or(false)
}

/// The sequence's fitted distance: how far from the centre of the box
/// `(lo, hi)` the camera stands at zoom 1 so that the box is inside the
/// frame at every one of `keys` -- whichever way that frame looks, through
/// whatever lens, at whatever zoom and pan. Frame `i` stands at this over
/// its zoom, so the frame that needs the most room is the one that fits
/// exactly and every other has room to spare.
///
/// With no camera keyframes this is `Camera::fit`'s distance, so an
/// animation that only moves parts is framed as a still of the same box is.
fn sequence_distance(base: &Camera, base_key: CameraKey, keys: &[Option<CameraKey>], (lo, hi): (Vec3, Vec3), margin: f64, aspect: f64) -> f64 {
    let centre = (lo + hi) * 0.5;
    let mut dist: f64 = 1e-3;
    for &key in keys {
        if places_eye(key) {
            continue;
        }
        let cam = turned(base, base_key, key, centre, 1.0);
        let need = cam.fit_distance(lo, hi, margin, aspect, pan_of(key));
        dist = dist.max(need * zoom_of(key));
    }
    dist
}

/// One frame's camera: the fitted base turned to `key` about the centre
/// of the fitted box, standing at the sequence's distance over the key's
/// zoom, the subject placed as the preset places it and then panned. With
/// no key, the base at that distance. A key that places the eye
/// (`view(eye=, target=)`) stands the camera exactly there,
/// looking at the target -- the base's when the key names none -- with
/// nothing fitted or placed.
fn frame_camera(base: &Camera, base_key: CameraKey, key: Option<CameraKey>, subject_height: f64, dist: f64, aspect: f64) -> Camera {
    if let Some(k) = key.filter(|_| places_eye(key)) {
        let mut cam = base.clone();
        if let Some(e) = k.eye {
            cam.eye = e;
        }
        if let Some(t) = k.target {
            cam.target = t;
        }
        let dir = (cam.target - cam.eye).normalized();
        cam.up = if dir.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
        if let Some(f) = k.fov {
            cam.fov_degrees = f;
        }
        return cam;
    }
    let mut cam = turned(base, base_key, key, base.target, dist / zoom_of(key));
    cam.place_subject(subject_height);
    let (px, py) = pan_of(key);
    cam.pan(px, py, aspect);
    cam
}

fn has_error(ds: &[Diagnostic]) -> bool {
    ds.iter().any(|d| d.severity == Severity::Error)
}

fn warning(message: String) -> pb::Diagnostic {
    pb::Diagnostic { severity: pb::Severity::Warning as i32, message, ..Default::default() }
}

/// Render an animation a document declares.
pub fn animate(req: &pb::AnimateRequest) -> anyhow::Result<pb::AnimateResponse> {
    let total = Stopwatch::start();
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("animate: no document"))?;
    let format = if req.format.is_empty() { "apng".to_owned() } else { req.format.trim().to_ascii_lowercase() };
    if format == "gif" && req.transparent {
        anyhow::bail!(
            "animate: a GIF pixel is either opaque or fully transparent, so it cannot carry an antialiased edge; \
             write --format apng or --format frames for a transparent background, or give the GIF a --background colour"
        );
    }
    if !FORMATS.contains(&format.as_str()) {
        anyhow::bail!(
            "animate: unknown format {:?}; expected one of {}",
            req.format,
            FORMATS.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ")
        );
    }
    // The look and the size bounds are the render request's: one clamp, one
    // options builder, one set of warnings.
    let mut render_req = render_request_of(req);
    let mut diagnostics = limits::clamp_render(&mut render_req);
    let look = Look::from_render(&render_req)?;
    let opts: RenderOptions = render_options_from_proto(&render_req);
    let (width, height) = (opts.width, opts.height);
    let aspect = width.max(1) as f64 / height.max(1) as f64;
    let mut frames_wanted = req.frames as usize;
    if frames_wanted > MAX_FRAMES {
        diagnostics.push(warning(format!("frames {} clamped to {MAX_FRAMES}", req.frames)));
        frames_wanted = MAX_FRAMES;
    }

    // One memo for every evaluation of this animation.
    let memo = Arc::new(Memo::new());
    let base_overrides: HashMap<String, ParamValue> = overrides_from_proto(&req.param_overrides);
    let evaluate = |overrides: &HashMap<String, ParamValue>| {
        evaluate_document_options(
            doc,
            overrides,
            req.tolerance,
            req.boolean_tolerance,
            &HashMap::new(),
            limits::default_limits(),
            EvalOptions { memo: Some(memo.clone()), preview: req.preview, solve_jobs: false },
        )
    };

    // The first evaluation: the one that fills the memo, and the one that
    // says what the document declares.
    let (mut ctx0, _files, first_eval_ms) = evaluate(&base_overrides);
    diagnostics.extend(ctx0.diagnostics.iter().map(Diagnostic::to_proto));
    let animations: Vec<pb::AnimationInfo> = ctx0.animations.iter().map(|a| a.to_proto()).collect();
    let stem = if doc.name.is_empty() { "animation".to_owned() } else { doc.name.clone() };
    let listing = pb::AnimateResponse {
        format: format.clone(),
        animations: animations.clone(),
        diagnostics: diagnostics.clone(),
        width,
        height,
        ..Default::default()
    };
    if has_error(&ctx0.diagnostics) || req.list {
        return Ok(pb::AnimateResponse { timings: Some(pb::AnimationTimings { total_ms: total.elapsed_ms(), first_eval_ms, ..Default::default() }), ..listing });
    }
    let decl: AnimationDecl = {
        let names: Vec<&str> = ctx0.animations.iter().map(|a| a.name.as_str()).collect();
        let found = if req.name.is_empty() {
            match ctx0.animations.len() {
                0 => anyhow::bail!("animate: the document declares no animation (declare one with animation(name, keyframes); docs/starlark-api.md)"),
                1 => Some(&ctx0.animations[0]),
                _ => anyhow::bail!("animate: the document declares several animations; name one with --name: {}", names.join(", ")),
            }
        } else {
            ctx0.animations.iter().find(|a| a.name == req.name)
        };
        match found {
            Some(a) => a.clone(),
            None => anyhow::bail!(
                "animate: no animation named {:?}; the document declares {}",
                req.name,
                if names.is_empty() { "none".to_owned() } else { names.join(", ") }
            ),
        }
    };
    if frames_wanted == 0 {
        let n = decl.uncapped_frame_count(req.fps);
        if n > MAX_FRAMES {
            let rate = if req.fps > 0.0 { req.fps } else { decl.fps };
            diagnostics.push(warning(format!(
                "animation {:?}: {} s at {} fps is {n} frames, over the {MAX_FRAMES}-frame cap; resampled to {MAX_FRAMES} frames over the whole {} s",
                decl.name,
                cadpole_core::context::fmt_num(decl.duration()),
                cadpole_core::context::fmt_num(rate),
                cadpole_core::context::fmt_num(decl.duration()),
            )));
        }
    }
    let times = decl.frame_times(req.fps, frames_wanted);
    // Over the cap this is the duration divided out, not 1/fps, so the file
    // plays for the animation's length (`AnimationDecl::frame_seconds`).
    let frame_seconds = decl.frame_seconds(req.fps, frames_wanted);
    let fps = if frame_seconds > 0.0 { 1.0 / frame_seconds } else { decl.fps };
    // The document's own value of every parameter: what a keyframe leaves
    // unsaid before its first mention.
    let document_values: HashMap<String, ParamValue> = ctx0.params.iter().map(|(d, v)| (d.name.clone(), v.clone())).collect();
    let document = |name: &str| document_values.get(name).cloned();
    // The display layers: the request's `--layer` overrides, with
    // whatever the keyframes' `view(layers=)` says at `t` written over them.
    let base_layers = crate::layer_overrides_from_proto(&req.layers);
    let layers_at = |t: f64| -> Vec<cadpole_core::layers::LayerOverride> {
        if decl.has_layers() {
            cadpole_core::layers::merge_overrides(&base_layers, &decl.sample(t, &document, None).layers)
        } else {
            base_layers.clone()
        }
    };
    let overrides_at = |t: f64| {
        let mut o = base_overrides.clone();
        for (n, v) in decl.sample(t, &document, None).params {
            o.insert(n, v);
        }
        o
    };

    // The camera: fitted once, to the union of the scenes at every keyframe.
    let fit = Stopwatch::start();
    let mut union: Option<(Vec3, Vec3)> = None;
    let mut fit_evals = 0u32;
    for k in &decl.keyframes {
        let o = overrides_at(k.t);
        let layers = layers_at(k.t);
        let bbox = if o == base_overrides {
            scene_from_context(&mut ctx0, &req.body_ids, &[], &layers).bbox
        } else {
            let (mut ctx, _, _) = evaluate(&o);
            fit_evals += 1;
            if has_error(&ctx.diagnostics) {
                diagnostics.extend(ctx.diagnostics.iter().map(Diagnostic::to_proto));
                return Ok(pb::AnimateResponse { diagnostics, ..listing });
            }
            scene_from_context(&mut ctx, &req.body_ids, &[], &layers).bbox
        };
        if let Some((lo, hi)) = bbox {
            union = Some(match union {
                None => (lo, hi),
                Some((a, b)) => (a.min(lo), b.max(hi)),
            });
        }
    }
    let fit_ms = fit.elapsed_ms();
    let (lo, hi) = union.unwrap_or((Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0)));
    // The request's camera, or the document's free camera when the
    // request names neither a preset nor an eye. A free camera stands
    // where it was put: nothing is fitted to the union.
    let doc_cam = document_camera(&req.camera, ctx0.render_defaults.as_ref());
    let free = is_free_camera(&doc_cam);
    let cam_req = doc_cam.clone().unwrap_or_default();
    let preset = if cam_req.preset.is_empty() { "iso".to_owned() } else { cam_req.preset.clone() };
    let margin = if cam_req.fit_margin > 0.0 { cam_req.fit_margin } else { Camera::preset_margin(&preset) };
    let mut base = camera_from_proto_aspect(&doc_cam, lo, hi, aspect);
    if !free {
        base.fit(lo, hi, margin, aspect);
    }
    let base_key = preset_key(&preset, base.fov_degrees);
    let subject_height = if free { 0.5 } else { Camera::preset_subject_height(&preset) };
    let key_at = |t: f64| if decl.has_camera() { decl.sample(t, &document, Some(base_key)).camera } else { None };
    // The distance every frame stands at, over its zoom: fitted to the
    // union box for every frame's direction, lens, zoom and pan at this
    // aspect ratio, so a camera that turns or pushes in never crops what
    // the keyframes show. A free camera stands where it was put.
    let dist = if free {
        (base.eye - base.target).length()
    } else {
        let keys: Vec<Option<CameraKey>> = times.iter().map(|&t| key_at(t)).collect();
        sequence_distance(&base, base_key, &keys, (lo, hi), margin, aspect)
    };
    let camera_for = |t: f64| frame_camera(&base, base_key, key_at(t), subject_height, dist, aspect);

    // The frames. An APNG delay is a fraction, so a whole frame rate is
    // exact (1/30); a GIF's is hundredths of a second, so 30 fps is 3 and 4
    // alternating -- each frame's delay is the cumulative time rounded,
    // less what the frames before it already took -- and the animation
    // keeps its length rather than playing a tenth fast. A browser treats
    // a GIF delay under 2 cs as 10, so 2 is the floor.
    let loop_forever = decl.looped;
    let delay = {
        let fps = 1.0 / frame_seconds;
        if fps.is_finite() && (fps - fps.round()).abs() < 1e-9 && fps >= 1.0 && fps <= 65535.0 {
            (1u16, fps.round() as u16)
        } else {
            ((frame_seconds * 1000.0).round().clamp(1.0, 65535.0) as u16, 1000u16)
        }
    };
    let delay_cs = |i: usize| -> u16 {
        let before = (i as f64 * frame_seconds * 100.0).round();
        let after = ((i + 1) as f64 * frame_seconds * 100.0).round();
        (after - before).max(2.0) as u16
    };
    let mut apng = if format == "apng" { Some(anim::ApngEncoder::new(width, height, loop_forever)) } else { None };
    let mut pngs: Vec<Vec<u8>> = Vec::new();
    let mut frame_stats = Vec::with_capacity(times.len());
    let mut cameras = Vec::with_capacity(times.len());
    let mut later_eval_ms = 0.0;
    let mut render_ms = 0.0;
    let mut last_memo: Option<pb::MemoReport> = None;
    for (i, &t) in times.iter().enumerate() {
        let o = overrides_at(t);
        let (mut ctx, _, eval_ms) = evaluate(&o);
        if has_error(&ctx.diagnostics) {
            diagnostics.push(warning(format!("frame {i} (t={}) did not build; the animation stops here", cadpole_core::context::fmt_num(t))));
            diagnostics.extend(ctx.diagnostics.iter().map(Diagnostic::to_proto));
            return Ok(pb::AnimateResponse { diagnostics, ..listing });
        }
        later_eval_ms += eval_ms;
        // The scene first: a display tessellation happens when the scene
        // asks for the mesh, so the counts are read after it.
        let scene: RenderScene = scene_from_context(&mut ctx, &req.body_ids, &[], &layers_at(t));
        let stat = pb::AnimationFrameStat {
            t,
            eval_ms,
            render_ms: 0.0,
            ops_run: ctx.memo_counts.runs.len() as u32,
            ops_hit: ctx.memo_counts.hits.len() as u32,
            tessellations: ctx.stats.tessellations,
            bakes: ctx.stats.bakes,
        };
        last_memo = Some(ctx.memo_report());
        // The frame's own sun: a `render_defaults(sun=sun(...,
        // hour))` over an animated `hour` parameter is a day's sweep.
        let mut frame_opts = opts.clone();
        apply_document_defaults(&mut frame_opts, &look, ctx.render_defaults.as_ref());
        let tolerance = ctx.tolerance;
        drop(ctx);
        let mut scene = scene;
        if let Some(sec) = &look.section {
            for w in section_scene(&mut scene, sec, tolerance) {
                if i == 0 {
                    diagnostics.push(warning(w));
                }
            }
        }
        let camera = camera_for(t);
        cameras.push(crate::camera_info(&camera, &scene, width, height, i as u32, t));
        let r = Stopwatch::start();
        let (img, _) = render_scene_stats(&scene, &camera, &frame_opts);
        drop(scene);
        let this_render = r.elapsed_ms();
        render_ms += this_render;
        frame_stats.push(pb::AnimationFrameStat { render_ms: this_render, ..stat });
        match &mut apng {
            Some(enc) => enc.push(&img, delay).map_err(|e| anyhow::anyhow!(e))?,
            None => pngs.push(img.to_png()),
        }
    }

    // The file.
    let enc = Stopwatch::start();
    let mut frames_out = Vec::new();
    let data = match format.as_str() {
        "apng" => apng.take().map(anim::ApngEncoder::finish).unwrap_or_default(),
        "gif" => {
            let n = pngs.len();
            let mut frame = |i: usize| -> Image { anim::decode_png(&pngs[i]).expect("a frame this run wrote decodes") };
            anim::encode_gif(width, height, n, &mut frame, &delay_cs, loop_forever).map_err(|e| anyhow::anyhow!(e))?
        }
        _ => {
            frames_out = pngs
                .into_iter()
                .enumerate()
                .map(|(i, png)| pb::AnimationFrame { t: times[i], index: i as u32, png })
                .collect();
            Vec::new()
        }
    };
    let encode_ms = enc.elapsed_ms();
    let suggested_filename = match format.as_str() {
        "apng" => format!("{stem}_{}.apng", decl.name),
        "gif" => format!("{stem}_{}.gif", decl.name),
        _ => format!("{stem}_{}", decl.name),
    };
    let first = camera_for(times[0]);
    let _ = fit_evals;
    Ok(pb::AnimateResponse {
        bytes: data.len() as u64,
        data,
        format,
        suggested_filename,
        frames: frames_out,
        width,
        height,
        frame_count: times.len() as u32,
        duration_s: decl.duration(),
        fps,
        r#loop: loop_forever,
        timings: Some(pb::AnimationTimings {
            total_ms: total.elapsed_ms(),
            first_eval_ms,
            later_eval_ms,
            render_ms,
            encode_ms,
            fit_ms,
        }),
        frame_stats,
        memo: last_memo,
        animations,
        diagnostics,
        cameras,
        camera: Some(pb::Camera {
            preset: preset.clone(),
            eye: Some(first.eye.to_proto()),
            target: Some(first.target.to_proto()),
            up: Some(first.up.to_proto()),
            fov_degrees: first.fov_degrees,
            orthographic: first.orthographic,
            fit_margin: margin,
            shift: first.shift,
            level: false,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> pb::Document {
        crate::document_from_scripts("t", vec![("main.star".into(), src.into())])
    }

    /// Two boxes as parts, one lifted by a parameter, and an animation that
    /// lowers it while a second parameter changes the other's height.
    const SCRIPT: &str = "lift = param(\"lift\", 0.0, unit=\"mm\", min=0, max=50)\n\
                          tall = param(\"tall\", 10.0, unit=\"mm\", min=5, max=30)\n\
                          a = part(box(10, 10, tall, name=\"a\"), name=\"a_p\")\n\
                          b = part(box(6, 6, 4, at=(2, 2, 10), name=\"b\"), name=\"b_p\")\n\
                          ai = instance(a, name=\"ai\")\n\
                          bi = instance(b, name=\"bi\")\n\
                          asm = assembly([ai, bi], name=\"asm\")\n\
                          explode(bi, (0, 0, lift))\n\
                          animation(\"drop\", [keyframe(0, lift=50), keyframe(1, lift=0)], fps=4)\n\
                          animation(\"grow\", [keyframe(0, tall=10), keyframe(1, tall=20)], fps=2, loop=True)\n";

    /// A listing names what the document declares and renders nothing.
    #[test]
    fn list_names_the_animations() {
        let r = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), list: true, ..Default::default() }).unwrap();
        let names: Vec<&str> = r.animations.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["drop", "grow"]);
        assert_eq!(r.animations[0].frames, 5);
        assert_eq!(r.animations[0].params, ["lift"]);
        assert_eq!(r.animations[1].r#loop, true);
        assert!(r.data.is_empty() && r.frames.is_empty() && r.frame_count == 0);
        assert!(!r.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error));
    }

    /// The frame count follows the rate, the fps override and the
    /// resample; a pose-only animation runs zero kernel ops and zero
    /// tessellations after its first frame; the APNG's first frame is a
    /// PNG of the right size; the memo is shared (hits, no re-runs).
    #[test]
    fn a_pose_animation_costs_no_kernel_work_after_the_first_frame() {
        let r = animate(&pb::AnimateRequest {
            document: Some(doc(SCRIPT)),
            name: "drop".into(),
            width: 64,
            height: 48,
            draft: true,
            ..Default::default()
        })
        .unwrap();
        assert!(!r.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", r.diagnostics);
        assert_eq!((r.frame_count, r.width, r.height, r.format.as_str()), (5, 64, 48, "apng"));
        assert_eq!(r.fps, 4.0);
        assert_eq!(r.duration_s, 1.0);
        assert!(!r.r#loop);
        assert_eq!(r.bytes as usize, r.data.len());
        let still = anim::decode_png(&r.data).unwrap();
        assert_eq!((still.width, still.height), (64, 48));
        assert_eq!(r.frame_stats.len(), 5);
        let (t0, t4) = (r.frame_stats[0].t, r.frame_stats[4].t);
        assert_eq!((t0, t4), (0.0, 1.0));
        // Every frame: the boxes' two extrudes and two tessellations are
        // memo hits; nothing runs; the two instance bodies are re-baked.
        for (i, s) in r.frame_stats.iter().enumerate() {
            assert_eq!((s.ops_run, s.tessellations), (0, 0), "frame {i}: {s:?}");
            assert!(s.ops_hit >= 2, "frame {i}: {s:?}");
            assert_eq!(s.bakes, 2, "frame {i}");
        }
        let m = r.memo.unwrap();
        assert!(m.entries >= 2 && m.ops_hit >= 2, "{m:?}");
        let t = r.timings.unwrap();
        assert!(t.first_eval_ms > 0.0 && t.render_ms > 0.0 && t.total_ms >= t.render_ms);

        // An fps override and a resample.
        let r = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), name: "drop".into(), width: 32, height: 24, draft: true, fps: 10.0, ..Default::default() }).unwrap();
        assert_eq!((r.frame_count, r.fps), (11, 10.0));
        let r = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), name: "drop".into(), width: 32, height: 24, draft: true, frames: 3, ..Default::default() }).unwrap();
        assert_eq!(r.frame_count, 3);
        assert_eq!(r.frame_stats.iter().map(|s| s.t).collect::<Vec<_>>(), vec![0.0, 0.5, 1.0]);
        assert_eq!(r.fps, 2.0);
    }

    /// GIF and frames formats; a looping animation stops one short of the
    /// end; a parameter that reaches into geometry re-runs its ops (and
    /// only its ops).
    #[test]
    fn gif_and_frames_and_a_geometry_parameter() {
        let r = animate(&pb::AnimateRequest {
            document: Some(doc(SCRIPT)),
            name: "grow".into(),
            format: "gif".into(),
            width: 32,
            height: 24,
            draft: true,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(&r.data[..6], b"GIF89a");
        assert_eq!(r.frame_count, 2, "a 1 s loop at 2 fps: t = 0 and 0.5");
        assert!(r.r#loop);
        assert_eq!(r.suggested_filename, "t_grow.gif");
        // Frame 0 is at the first evaluation's state (tall=10, a hit); frame
        // 1 changes `tall`, so box a's extrude and its display tessellation
        // run again (two memoised ops, one of them a tessellation) while
        // box b's two are hits.
        assert_eq!((r.frame_stats[0].ops_run, r.frame_stats[0].tessellations), (0, 0));
        assert_eq!((r.frame_stats[1].ops_run, r.frame_stats[1].tessellations), (2, 1), "{:?}", r.frame_stats[1]);
        assert_eq!(r.frame_stats[1].ops_hit, 2);

        let r = animate(&pb::AnimateRequest {
            document: Some(doc(SCRIPT)),
            name: "drop".into(),
            format: "frames".into(),
            width: 32,
            height: 24,
            draft: true,
            frames: 2,
            ..Default::default()
        })
        .unwrap();
        assert!(r.data.is_empty() && r.bytes == 0);
        assert_eq!(r.frames.len(), 2);
        assert_eq!((r.frames[1].index, r.frames[1].t), (1, 1.0));
        assert!(r.frames[0].png.starts_with(b"\x89PNG"));
        assert_eq!(r.suggested_filename, "t_drop");
    }

    /// The refusals: no document, an unknown format, an unknown name, a
    /// document with several animations and no name, a document with none.
    #[test]
    fn refusals() {
        let e = animate(&pb::AnimateRequest::default()).unwrap_err().to_string();
        assert!(e.contains("no document"), "{e}");
        let e = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), format: "webm".into(), ..Default::default() }).unwrap_err().to_string();
        assert!(e.contains("unknown format \"webm\"") && e.contains("\"apng\", \"gif\", \"frames\""), "{e}");
        let e = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), name: "spin".into(), ..Default::default() }).unwrap_err().to_string();
        assert!(e.contains("no animation named \"spin\"") && e.contains("drop, grow"), "{e}");
        let e = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), ..Default::default() }).unwrap_err().to_string();
        assert!(e.contains("several animations") && e.contains("--name"), "{e}");
        let e = animate(&pb::AnimateRequest { document: Some(doc("b = box(1, 1, 1)\n")), ..Default::default() }).unwrap_err().to_string();
        assert!(e.contains("declares no animation"), "{e}");
        // A document that does not build: diagnostics, nothing rendered.
        let r = animate(&pb::AnimateRequest { document: Some(doc("b = box(-1, 1, 1)\nanimation(\"a\", [keyframe(0)])\n")), ..Default::default() }).unwrap();
        assert!(r.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error));
        assert!(r.data.is_empty() && r.frame_count == 0);
    }

    /// The camera is fixed across frames — fitted to the union of the
    /// keyframes' scenes, so the first frame (the box lifted 50 mm) and the
    /// last (dropped) are framed alike — and a camera keyframe turns it
    /// about the same target, from the request's preset, the short way
    /// round.
    #[test]
    fn the_camera_is_fixed_by_the_bbox_union_and_keyframes_turn_it() {
        // The union of the two keyframes' scenes: the box at z 10..14 lifted
        // to 60..64 at t=0 and at rest at t=1, so the union's top is 64.
        let r = animate(&pb::AnimateRequest { document: Some(doc(SCRIPT)), name: "drop".into(), width: 32, height: 24, draft: true, frames: 2, camera: Some(pb::Camera { preset: "front".into(), ..Default::default() }), ..Default::default() }).unwrap();
        let cam = r.camera.unwrap();
        let target = cam.target.unwrap();
        // The fitted target is the union's centre before the subject is
        // placed: x and y at the boxes' middle, z at half of 64.
        assert!((target.x - 5.0).abs() < 1e-9 && (target.y - 5.0).abs() < 1e-9, "{target:?}");
        assert!((target.z - 32.0).abs() < 1e-9, "the union of both keyframes' bboxes, not one frame's: {target:?}");
        let eye = cam.eye.unwrap();
        assert!(eye.y < target.y && (eye.x - target.x).abs() < 1e-9, "a front view looks along +y: {eye:?}");

        // A keyframed camera: az from the preset (front = 0) to 90 over the
        // animation, the eye staying at the fitted distance.
        let src = format!("{SCRIPT}animation(\"orbit\", [keyframe(0, lift=0), keyframe(1, lift=0, camera=view(az=90))], fps=2, ease=ease.linear)\n");
        let r = animate(&pb::AnimateRequest { document: Some(doc(&src)), name: "orbit".into(), format: "frames".into(), width: 32, height: 24, draft: true, camera: Some(pb::Camera { preset: "front".into(), ..Default::default() }), ..Default::default() }).unwrap();
        assert_eq!(r.frame_count, 3);
        let base_key = preset_key("front", 0.0);
        assert!((base_key.az.unwrap()).abs() < 1e-9 && base_key.el.unwrap().abs() < 1e-9, "{base_key:?}");
        assert_eq!(preset_key("iso", 0.0).az.map(|a| a.round()), Some(45.0));
        assert_eq!(preset_key("product", 0.0).round(), CameraKey { az: Some(30.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: Some(1.0), pan: Some((0.0, 0.0)) });
        let iso = preset_key("iso", 0.0);
        assert!((iso.el.unwrap() - 29.4964).abs() < 1e-3, "{iso:?}");
        // The frames differ (the camera moved) and the middle one is the
        // turn's midpoint.
        assert_ne!(r.frames[0].png, r.frames[2].png);
        let base = Camera { eye: Vec3::new(0.0, -100.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 35.0, orthographic: false, shift: 0.0 };
        let mid = frame_camera(&base, base_key, Some(CameraKey { az: Some(45.0), el: Some(0.0), fov: None, eye: None, target: None, zoom: None, pan: None }), 0.5, 100.0, 1.0);
        assert!((mid.eye.length() - 100.0).abs() < 1e-9, "the distance is kept: {:?}", mid.eye);
        assert!((mid.eye.x - 100.0 / 2f64.sqrt()).abs() < 1e-9 && (mid.eye.y + 100.0 / 2f64.sqrt()).abs() < 1e-9, "{:?}", mid.eye);
        let up = frame_camera(&base, base_key, Some(CameraKey { az: None, el: Some(90.0), fov: Some(20.0), eye: None, target: None, zoom: None, pan: None }), 0.5, 100.0, 1.0);
        assert!((up.eye.z - 100.0).abs() < 1e-9 && up.fov_degrees == 20.0 && up.up == Vec3::Y, "{up:?}");
        assert_eq!(frame_camera(&base, base_key, None, 0.5, 100.0, 1.0).eye, base.eye, "no key: the base as it is");
    }

    /// A frame's sidecar camera, read the way a page reads it: a world
    /// point through `projection * view` to a pixel.
    fn pixel(c: &pb::CameraInfo, p: [f64; 3]) -> (f64, f64) {
        let mul = |m: &[f64], v: [f64; 4]| -> [f64; 4] { std::array::from_fn(|r| (0..4).map(|k| m[r * 4 + k] * v[k]).sum()) };
        let clip = mul(&c.projection, mul(&c.view, [p[0], p[1], p[2], 1.0]));
        assert!(clip[3] > 0.0, "in front of the camera");
        ((clip[0] / clip[3] + 1.0) * 0.5 * c.width as f64, (1.0 - clip[1] / clip[3]) * 0.5 * c.height as f64)
    }

    fn v3(v: &Option<pb::Vec3>) -> Vec3 {
        let v = v.as_ref().unwrap();
        Vec3::new(v.x, v.y, v.z)
    }

    /// An explode with the camera drifting round, pushing in and sliding:
    /// every key is honoured, and the whole exploded subject is inside the
    /// frame at every frame, in a landscape and in a portrait image alike.
    #[test]
    fn a_drifting_zooming_camera_keeps_the_subject_in_frame_at_either_aspect() {
        let src = format!(
            "{SCRIPT}animation(\"push\", [keyframe(0, lift=0, camera=view(az=20, el=15)), \
             keyframe(1, lift=50, camera=view(az=32, el=22, zoom=1.15, pan=(0.04, -0.03)))], fps=4, ease=ease.linear)\n"
        );
        // What the keyframes show between them: both boxes, the small one
        // lifted 50, so x and y 0..10 and z 0..64.
        let (lo, hi) = ([0.0, 0.0, 0.0], [10.0, 10.0, 64.0]);
        let mut tightest = [0.0f64; 2];
        for (n, (w, h)) in [(96u32, 64u32), (64, 96)].into_iter().enumerate() {
            let r = animate(&pb::AnimateRequest { document: Some(doc(&src)), name: "push".into(), format: "frames".into(), width: w, height: h, draft: true, ..Default::default() }).unwrap();
            assert_eq!(r.frame_count, 5);
            assert_eq!(r.cameras.len(), 5, "one camera per frame");
            let (first, last) = (&r.cameras[0], &r.cameras[4]);
            assert_eq!((first.index, last.index, last.t), (0, 4, 1.0));
            // The direction: the keyed azimuth and elevation at each end.
            for (c, az, el) in [(first, 20.0, 15.0), (last, 32.0, 22.0)] {
                let d = (v3(&c.eye) - v3(&c.target)).normalized();
                assert!((d - direction(az, el)).length() < 1e-9, "az={az} el={el}: {d:?}");
            }
            // The zoom: the last frame stands 1.15 times closer than the first.
            let dist = |c: &pb::CameraInfo| (v3(&c.eye) - v3(&c.target)).length();
            assert!((dist(first) / dist(last) - 1.15).abs() < 1e-9, "{} / {}", dist(first), dist(last));
            assert!(dist(&r.cameras[2]) < dist(first) && dist(&r.cameras[2]) > dist(last), "and eases between");
            // The pan: the fitted centre sits at the middle of the first
            // frame, and 4 % left and 3 % up of it in the last.
            let centre = [5.0, 5.0, 32.0];
            let (x0, y0) = pixel(first, centre);
            assert!((x0 - w as f64 * 0.5).abs() < 1e-6 && (y0 - h as f64 * 0.5).abs() < 1e-6, "({x0}, {y0})");
            let (x1, y1) = pixel(last, centre);
            assert!((x1 - w as f64 * 0.46).abs() < 1e-6 && (y1 - h as f64 * 0.47).abs() < 1e-6, "({x1}, {y1})");
            // The subject: every corner of the union box inside every frame,
            // and the frame's own bounding box (the sidecar's) likewise.
            for c in &r.cameras {
                for i in 0..8 {
                    let p = [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
                    let (x, y) = pixel(c, p);
                    assert!(x >= 0.0 && x <= w as f64 && y >= 0.0 && y <= h as f64, "{w}x{h} frame {}: corner {p:?} at ({x:.1}, {y:.1})", c.index);
                    tightest[n] = tightest[n].max(((x - w as f64 * 0.5).abs() / (w as f64 * 0.5)).max((y - h as f64 * 0.5).abs() / (h as f64 * 0.5)));
                }
                let b = &c.bounds_px;
                assert_eq!(b.len(), 4);
                assert!(b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= w as f64 && b[3] <= h as f64, "frame {}: {b:?}", c.index);
            }
            // Deterministic: the same request draws the same frames.
            let again = animate(&pb::AnimateRequest { document: Some(doc(&src)), name: "push".into(), format: "frames".into(), width: w, height: h, draft: true, ..Default::default() }).unwrap();
            assert!(r.frames.iter().zip(&again.frames).all(|(a, b)| a.png == b.png));
        }
        // The fit is to the frame that needs the most room, not a loose
        // bound: at each aspect some corner comes close to an edge.
        assert!(tightest[0] > 0.8 && tightest[1] > 0.8, "{tightest:?}");
    }

    /// A cut that is there from the first frame: a bool parameter set at
    /// keyframe 0 holds for the whole sequence while another parameter
    /// moves, and a request's `section` cuts every frame alike.
    #[test]
    fn a_cutaway_holds_from_the_first_frame_while_the_explode_moves() {
        let src = "lift = param(\"lift\", 0.0, unit=\"mm\", min=0, max=50)\n\
                   cut = param(\"cutaway\", False)\n\
                   a = part(box(5 if cut else 10, 10, 10, name=\"a\"), name=\"a_p\")\n\
                   b = part(box(4, 4, 4, at=(0, 0, 10), name=\"b\"), name=\"b_p\")\n\
                   bi = instance(b, name=\"bi\")\n\
                   asm = assembly([instance(a, name=\"ai\"), bi], name=\"asm\")\n\
                   explode(bi, (0, 0, lift))\n\
                   animation(\"open\", [keyframe(0, cutaway=True, lift=0), keyframe(1, lift=50)], fps=4)\n\
                   animation(\"lift\", [keyframe(0, lift=0), keyframe(1, lift=50)], fps=4)\n";
        let r = animate(&pb::AnimateRequest { document: Some(doc(src)), name: "open".into(), format: "frames".into(), width: 32, height: 24, draft: true, ..Default::default() }).unwrap();
        assert_eq!(r.cameras.len(), 5);
        for c in &r.cameras {
            assert!((v3(&c.bounds_max).x - 5.0).abs() < 1e-6, "frame {} is cut: {:?}", c.index, c.bounds_max);
        }
        assert!(v3(&r.cameras[4].bounds_max).z > v3(&r.cameras[0].bounds_max).z + 40.0, "and the explode still moves");
        let r = animate(&pb::AnimateRequest { document: Some(doc(src)), name: "lift".into(), format: "frames".into(), width: 32, height: 24, draft: true, section: "x=3".into(), ..Default::default() }).unwrap();
        for c in &r.cameras {
            assert!((v3(&c.bounds_max).x - 3.0).abs() < 1e-3, "frame {} is sectioned: {:?}", c.index, c.bounds_max);
        }
    }

    /// A transparent animation: frames and an APNG carry coverage alpha, a
    /// GIF is refused with the reason.
    #[test]
    fn transparent_frames_carry_alpha_and_a_gif_is_refused() {
        let req = |format: &str| pb::AnimateRequest { document: Some(doc(SCRIPT)), name: "drop".into(), format: format.into(), width: 48, height: 36, frames: 2, no_grid: true, transparent: true, ..Default::default() };
        let r = animate(&req("frames")).unwrap();
        let img = Image::from_png(&r.frames[0].png).unwrap();
        let alpha: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(alpha[0], 0, "the corner is empty");
        assert!(alpha.iter().any(|&a| a == 255), "the part is opaque");
        assert!(alpha.iter().any(|&a| a > 0 && a < 255), "and its edge is partly covered");
        let apng = animate(&req("apng")).unwrap();
        let first = Image::from_png(&apng.data).unwrap();
        assert_eq!(first.rgba[3], 0, "the APNG's first frame keeps its alpha");
        let e = animate(&req("gif")).unwrap_err().to_string();
        assert!(e.contains("GIF") && e.contains("apng"), "{e}");
    }

    trait Round {
        fn round(self) -> Self;
    }
    impl Round for CameraKey {
        fn round(self) -> CameraKey {
            CameraKey { az: self.az.map(|v| (v * 1e6).round() / 1e6), el: self.el.map(|v| (v * 1e6).round() / 1e6), ..self }
        }
    }
}
