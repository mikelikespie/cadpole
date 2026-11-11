//! Cadpole's GPU renderer: one `wgpu`/WGSL implementation of the viewport for
//! WebGPU in the browser, Metal on macOS and iOS, and Vulkan on Linux.
//!
//! The entry point that matters first is headless: [`Renderer::render`] takes
//! the same `(items, camera, options)` as `cadpole_core::render::render` and
//! returns the same [`Image`], rendered on the GPU into an offscreen texture
//! and read back. That is what makes the look testable — a GPU render and a
//! CPU render of the same scene can be diffed in CI on a software adapter —
//! and it is what `cadpole render --backend gpu` uses.
//!
//! The passes mirror the CPU rasterizer's stages one for one, so the two can
//! be kept in step (the module comments say which function each is a port of):
//!
//! | Pass | Target | CPU counterpart |
//! | --- | --- | --- |
//! | geometry | normal + ids + depth, MSAA | `render/aa.rs::geometry_pass` |
//! | translucent | a second G-buffer, MSAA | `render/oit.rs` (one layer) |
//! | ssao + blur | R16Float | `render/occlusion.rs::ssao` |
//! | shade + tonemap + resolve | Rgba16Float | `shading::shade`, `aa::tonemap`, `aa::accumulate` |
//! | grid, edges, silhouettes, highlights | ink, then composited | `render/lines.rs` |
//! | encode | Rgba8Unorm, read back | `aa::resolve` |
//! | id | Rg32Uint, read back by [`Renderer::pick`] | replaces the CPU raycast |
//!
//! Anti-aliasing is hardware MSAA rather than the CPU's jittered camera
//! passes, but the resolve is the same: every sample is shaded and tonemapped
//! on its own and the *display-linear* results are averaged, never sRGB ones.
//!
//! What is deliberately not the same: translucency keeps only the nearest
//! layer (the CPU keeps a per-pixel list), and overlay ink behind glass is not
//! attenuated. Both are noted where they happen.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod diff;
mod gpu;
mod pipelines;
mod scene;
mod surface;
mod targets;
#[cfg(test)]
mod tests;

use bytemuck::{Pod, Zeroable};
use cadpole_core::geom::Vec3;
use cadpole_core::render::{Camera, Image, RenderOptions, RenderStats, HIGHLIGHT};

pub use gpu::{describe as describe_adapter, AdapterPreference};
pub use scene::PICK_EDGE;
pub use surface::{Demand, SurfaceSource, Viewport};

use gpu::Gpu;
use pipelines::Pipelines;
use scene::{Upload, AO_BIAS, AO_BLUR_DEPTH, AO_RADIUS, HF_SIZE, NO_ITEM};
use targets::Targets;

/// Feature-edge ink (`lines::EDGE_INK`, sRGB8 22/24/28) in display-linear.
const EDGE_INK: [f32; 3] = [0.00802319311, 0.00913405884, 0.0116122449];

#[derive(Debug)]
pub enum Error {
    /// No GPU at all: no Vulkan/Metal driver, no software rasterizer. Tests
    /// skip on this; `cadpole render --backend gpu` falls back to the CPU.
    NoAdapter(String),
    Device(String),
    Surface(String),
    /// The renderer was asked for something the platform cannot do yet.
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoAdapter(m) => write!(f, "no GPU adapter available: {m}"),
            Error::Device(m) => write!(f, "GPU device error: {m}"),
            Error::Surface(m) => write!(f, "surface error: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// How a [`Renderer`] is opened.
#[derive(Clone, Copy, Debug)]
pub struct RendererOptions {
    pub adapter: AdapterPreference,
    /// Read the id buffer back on every render so [`Renderer::pick`] can
    /// answer without another frame. On by default; a batch renderer that
    /// never picks can turn it off.
    pub picking: bool,
}

impl Default for RendererOptions {
    fn default() -> RendererOptions {
        RendererOptions { adapter: AdapterPreference::Auto, picking: true }
    }
}

impl RendererOptions {
    pub fn new() -> RendererOptions {
        RendererOptions::default()
    }
}

/// What `pick()` found under the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    /// Index into the item list the frame was rendered from.
    pub item: u32,
    /// Topology face index, or `None` when an edge was hit.
    pub face: Option<u32>,
    /// Index into that item's `edges`, when an edge was hit.
    pub edge: Option<u32>,
    /// Pixel distance from the requested point.
    pub distance: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    fwd: [f32; 4],
    viewport: [f32; 4],
    scene_lo: [f32; 4],
    scene_hi: [f32; 4],
    center: [f32; 4],
    background: [f32; 4],
    grid: [f32; 4],
    ao: [f32; 4],
    hf: [f32; 4],
    hf2: [f32; 4],
    counts: [u32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct OverlayParams {
    color: [f32; 4],
}

/// The renderer: a device, its pipelines, and the targets of the last size
/// rendered. Reuse one across frames; opening a device costs milliseconds.
pub struct Renderer {
    gpu: Gpu,
    pipelines: Option<Pipelines>,
    targets: Option<Targets>,
    picking: bool,
    /// The frame uniform of the last render, kept alive so the surface path
    /// can bind group 0 for its own blit pipeline.
    globals_bg: Option<wgpu::BindGroup>,
    /// The id buffer of the last frame, `(item, face_or_edge)` per pixel.
    pick_data: Vec<[u32; 2]>,
    pick_size: (u32, u32),
    info: wgpu::AdapterInfo,
}

impl Renderer {
    /// Open a device, blocking. `Err(Error::NoAdapter)` is the "this machine
    /// has no GPU" case a caller is expected to fall back on.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(opts: RendererOptions) -> Result<Renderer, Error> {
        gpu::block_on(Renderer::new_async(opts))
    }

    pub async fn new_async(opts: RendererOptions) -> Result<Renderer, Error> {
        let gpu = Gpu::open(opts.adapter).await?;
        Ok(Renderer::from_gpu(gpu, opts))
    }

    fn from_gpu(gpu: Gpu, opts: RendererOptions) -> Renderer {
        let info = gpu.adapter.get_info();
        Renderer {
            gpu,
            pipelines: None,
            targets: None,
            picking: opts.picking,
            globals_bg: None,
            pick_data: Vec::new(),
            pick_size: (0, 0),
            info,
        }
    }

    /// The adapter this renderer is running on: `"llvmpipe (Vulkan, Cpu)"`.
    pub fn adapter(&self) -> String {
        gpu::describe(&self.info)
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }

    pub fn instance(&self) -> &wgpu::Instance {
        &self.gpu.instance
    }

    pub fn adapter_handle(&self) -> &wgpu::Adapter {
        &self.gpu.adapter
    }

    /// The encoded sRGB8 image of the last frame, still on the GPU: what the
    /// surface path blits and what the headless path reads back.
    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.out)
    }

    /// Bind group layout of the single-texture passes (`encode`, `blit`), so a
    /// [`Viewport`] can build a pipeline for its own surface format.
    pub fn blit_layout(&self) -> Option<&wgpu::BindGroupLayout> {
        self.pipelines.as_ref().map(|p| &p.layouts.encode)
    }

    /// The pipeline layout those passes use (globals + one texture).
    pub fn pipeline_layouts(&self) -> Option<&wgpu::PipelineLayout> {
        self.pipelines.as_ref().map(|p| &p.encode_layout)
    }

    /// Whether the pipelines currently built are the multisampled variants.
    pub fn multisampled(&self) -> bool {
        self.pipelines.as_ref().is_some_and(|p| p.samples > 1)
    }

    /// Group 0 of the last frame. Valid only after a render.
    pub fn globals_bind_group(&self) -> &wgpu::BindGroup {
        self.globals_bg.as_ref().expect("render a frame before presenting one")
    }

    /// Render a frame and leave it in [`Renderer::output_texture`], with no
    /// read back. The surface path draws from there.
    pub fn render_to_texture(
        &mut self,
        items: &[cadpole_core::render::Item<'_>],
        cam: &Camera,
        opts: &RenderOptions,
    ) -> Result<(), Error> {
        self.prepare(items, cam, opts).map(|_| ())
    }

    pub fn set_picking(&mut self, on: bool) {
        self.picking = on;
    }

    /// Anti-aliasing samples actually used for an image of this size: the CPU
    /// renderer's snap to {1, 4, 8} and its 16 Mpx cut-off, then whatever the
    /// adapter supports for every multisampled format in the G-buffer.
    pub fn sample_count(&self, opts: &RenderOptions) -> u32 {
        let px = opts.width.max(1) as u64 * opts.height.max(1) as u64;
        let want = if px > 16_000_000 { 1 } else { cadpole_core::render::sample_pattern(opts.samples).len() as u32 };
        let mut n = want;
        while n > 1 && !self.supports_samples(n) {
            n /= 2;
        }
        n.max(1)
    }

    /// Whether the DEVICE accepts `n` samples for every multisampled G-buffer
    /// format. That is wgpu's own validation rule: the adapter's per-format
    /// capabilities only when the device was opened with
    /// TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES (`Gpu::from_adapter` asks for
    /// it when offered), else what WebGPU guarantees for the format. Asking
    /// the adapter alone can pick 8 on a device that allows only 4.
    fn supports_samples(&self, n: u32) -> bool {
        let device_features = self.gpu.device.features();
        let adapter_specific = device_features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        [pipelines::NORMAL, pipelines::IDS, pipelines::DEPTH].iter().all(|f| {
            let features = if adapter_specific {
                self.gpu.adapter.get_texture_format_features(*f)
            } else {
                f.guaranteed_format_features(device_features)
            };
            features.flags.sample_count_supported(n)
        })
    }

    /// Render to an offscreen texture and read it back. The result has the
    /// same shape as `cadpole_core::render::render`'s: sRGB8 RGBA, the background
    /// bit-exact where nothing covers it.
    pub fn render(&mut self, items: &[cadpole_core::render::Item<'_>], cam: &Camera, opts: &RenderOptions) -> Result<Image, Error> {
        Ok(self.render_with_stats(items, cam, opts)?.0)
    }

    /// [`Renderer::render`] plus timings, in the CPU renderer's `RenderStats`
    /// so `cadpole render --stats` can print either backend the same way.
    pub fn render_with_stats(
        &mut self,
        items: &[cadpole_core::render::Item<'_>],
        cam: &Camera,
        opts: &RenderOptions,
    ) -> Result<(Image, RenderStats), Error> {
        let (w, h) = (opts.width.max(1), opts.height.max(1));
        let t1 = now_ms();
        let samples = self.prepare(items, cam, opts)?;
        let t2 = now_ms();
        let img = self.read_image(w, h)?;
        if self.picking {
            self.read_pick(w, h)?;
        }
        let t3 = now_ms();
        let stats = RenderStats {
            geometry_ms: t2 - t1,
            resolve_ms: t3 - t2,
            shade_ms: 0.0,
            threads_used: 1,
            passes: samples,
            ..Default::default()
        };
        Ok((img, stats))
    }

    /// Build the scene, (re)build pipelines and targets for this size and
    /// sample count, and submit one frame. Returns the sample count used.
    fn prepare(
        &mut self,
        items: &[cadpole_core::render::Item<'_>],
        cam: &Camera,
        opts: &RenderOptions,
    ) -> Result<u32, Error> {
        let (w, h) = (opts.width.max(1), opts.height.max(1));
        let samples = self.sample_count(opts);
        let up = Upload::build(items, cam, opts);
        if self.pipelines.as_ref().map(|p| p.samples) != Some(samples) {
            self.pipelines = Some(Pipelines::new(&self.gpu.device, samples));
        }
        if !self.targets.as_ref().is_some_and(|t| t.matches(w, h, samples)) {
            self.targets = Some(Targets::new(&self.gpu.device, w, h, samples));
        }
        self.encode_frame(&up, cam, opts, samples)?;
        Ok(samples)
    }

    /// Everything from the G-buffer to the encoded image, in one submission.
    fn encode_frame(&mut self, up: &Upload, cam: &Camera, opts: &RenderOptions, samples: u32) -> Result<(), Error> {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let pipes = self.pipelines.as_ref().expect("pipelines");
        let t = self.targets.as_ref().expect("targets");
        let (w, h) = (t.width, t.height);

        let globals = build_globals(up, cam, opts, samples, w, h);
        let globals_buf = pipelines::buffer(device, "cadpole.globals", &[globals], wgpu::BufferUsages::UNIFORM);
        let materials = if up.materials.is_empty() {
            vec![scene::GpuMaterial { base: [0.0; 4], params: [0.0; 4], emissive: [0.0; 4] }]
        } else {
            up.materials.clone()
        };
        let materials_buf = pipelines::buffer(device, "cadpole.materials", &materials, wgpu::BufferUsages::STORAGE);
        let opaque_buf = pipelines::buffer(device, "cadpole.opaque", &up.opaque, wgpu::BufferUsages::VERTEX);
        let glass_buf = pipelines::buffer(device, "cadpole.translucent", &up.translucent, wgpu::BufferUsages::VERTEX);
        let crease_buf = pipelines::buffer(device, "cadpole.creases", &up.creases, wgpu::BufferUsages::VERTEX);
        let highlight_buf = pipelines::buffer(device, "cadpole.highlights", &up.highlights, wgpu::BufferUsages::VERTEX);
        let pick_edge_buf = pipelines::buffer(device, "cadpole.pick_edges", &up.pick_edges, wgpu::BufferUsages::VERTEX);

        // The baked contact shadow, or a single "no shadow" texel.
        let hf_size = if up.heightfield.enabled { HF_SIZE } else { 1 };
        let hf_data: Vec<f32> = if up.heightfield.enabled {
            up.heightfield.shadow.clone()
        } else {
            vec![1.0]
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t.contact,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&hf_data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(hf_size * 4),
                rows_per_image: Some(hf_size),
            },
            wgpu::Extent3d { width: hf_size, height: hf_size, depth_or_array_layers: 1 },
        );

        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadpole.globals"),
            layout: &pipes.layouts.globals,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: materials_buf.as_entire_binding() },
            ],
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cadpole.frame") });

        // --- G-buffer -----------------------------------------------------
        let clear_ids = wgpu::Color { r: NO_ITEM as f64, g: 0xffff as f64, b: 0xffff as f64, a: 0.0 };
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.geom"),
                color_attachments: &[
                    Some(color_att(&t.g_normal, wgpu::Color::TRANSPARENT)),
                    Some(color_att(&t.g_ids, clear_ids)),
                ],
                depth_stencil_attachment: Some(depth_att(&t.g_depth)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !up.opaque.is_empty() {
                pass.set_pipeline(&pipes.geom);
                pass.set_bind_group(0, &globals_bg, &[]);
                pass.set_vertex_buffer(0, opaque_buf.slice(..));
                pass.draw(0..up.opaque.len() as u32, 0..1);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.glass"),
                color_attachments: &[
                    Some(color_att(&t.t_normal, wgpu::Color::TRANSPARENT)),
                    Some(color_att(&t.t_ids, clear_ids)),
                ],
                depth_stencil_attachment: Some(depth_att(&t.t_depth)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !up.translucent.is_empty() {
                pass.set_pipeline(&pipes.glass);
                pass.set_bind_group(0, &globals_bg, &[]);
                pass.set_vertex_buffer(0, glass_buf.slice(..));
                pass.draw(0..up.translucent.len() as u32, 0..1);
            }
        }

        // --- ambient occlusion --------------------------------------------
        let ssao_bg = |src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cadpole.ssao"),
                layout: &pipes.layouts.ssao,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.g_depth) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.g_normal) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.g_ids) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(src) },
                ],
            })
        };
        if opts.ambient_occlusion {
            // The texture bound as the source of the raw pass is not read by
            // its shader, but it must not be that pass's own target: a texture
            // cannot be a resource and a colour attachment in one usage scope.
            for (label, pipe, target, src) in [
                ("cadpole.ssao", &pipes.ssao, &t.ao_raw, &t.ao),
                ("cadpole.blur_x", &pipes.blur_x, &t.ao_tmp, &t.ao_raw),
                ("cadpole.blur_y", &pipes.blur_y, &t.ao, &t.ao_tmp),
            ] {
                let bg = ssao_bg(src);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(color_att(target, wgpu::Color::WHITE))],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &globals_bg, &[]);
                pass.set_bind_group(1, &bg, &[]);
                pass.draw(0..3, 0..1);
            }
        } else {
            // Unoccluded everywhere: a clear is the whole pass.
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.ao_off"),
                color_attachments: &[Some(color_att(&t.ao, wgpu::Color::WHITE))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // --- shade, tonemap, resolve --------------------------------------
        {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cadpole.shade"),
                layout: &pipes.layouts.shade,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.g_depth) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.g_normal) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.g_ids) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.ao) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&t.contact_view) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&t.t_depth) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&t.t_normal) },
                    wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&t.t_ids) },
                ],
            });
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.shade"),
                color_attachments: &[Some(color_att(&t.display, wgpu::Color::TRANSPARENT))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipes.shade);
            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // --- overlay ------------------------------------------------------
        let params = |color: [f32; 3]| {
            pipelines::buffer(
                device,
                "cadpole.overlay_params",
                &[OverlayParams { color: [color[0], color[1], color[2], 1.0] }],
                wgpu::BufferUsages::UNIFORM,
            )
        };
        let overlay_bg = |ink: &wgpu::TextureView, params: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cadpole.overlay"),
                layout: &pipes.layouts.overlay,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.g_depth) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.g_ids) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.g_normal) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(ink) },
                    wgpu::BindGroupEntry { binding: 4, resource: params.as_entire_binding() },
                ],
            })
        };
        let no_params = params([0.0; 3]);
        let edge_params = params(EDGE_INK);
        let highlight_params = params(HIGHLIGHT);

        if opts.grid_mode() != cadpole_core::render::GridMode::Off {
            let bg = overlay_bg(&t.ink_a, &no_params);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.grid"),
                color_attachments: &[Some(load_att(&t.display))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipes.grid);
            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // `src` is unused by the edge shader but must not be the pass's own
        // target, so the two ink layers take turns.
        let draw_ink = |enc: &mut wgpu::CommandEncoder,
                        label: &'static str,
                        target: &wgpu::TextureView,
                        src: &wgpu::TextureView,
                        segs: u32,
                        buf: &wgpu::Buffer| {
            let bg = overlay_bg(src, &no_params);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(color_att(target, wgpu::Color::TRANSPARENT))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if segs > 0 {
                pass.set_pipeline(&pipes.edge);
                pass.set_bind_group(0, &globals_bg, &[]);
                pass.set_bind_group(1, &bg, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..segs);
            }
        };
        let composite = |enc: &mut wgpu::CommandEncoder, label: &'static str, ink: &wgpu::TextureView, params: &wgpu::Buffer| {
            let bg = overlay_bg(ink, params);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(load_att(&t.display))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipes.composite);
            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &bg, &[]);
            pass.draw(0..3, 0..1);
        };

        let edges_on = opts.edge_mode() != cadpole_core::render::EdgeMode::Off;
        if edges_on || !up.highlights.is_empty() {
            draw_ink(&mut enc, "cadpole.creases", &t.ink_a, &t.ink_b, up.creases.len() as u32, &crease_buf);
            if edges_on && opts.draw_silhouettes {
                // Outlines run after the creases so they can stand back where
                // a feature edge already draws the same line.
                let bg = overlay_bg(&t.ink_a, &no_params);
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("cadpole.silhouette"),
                        color_attachments: &[Some(color_att(&t.ink_b, wgpu::Color::TRANSPARENT))],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&pipes.silhouette);
                    pass.set_bind_group(0, &globals_bg, &[]);
                    pass.set_bind_group(1, &bg, &[]);
                    pass.draw(0..3, 0..1);
                }
                composite(&mut enc, "cadpole.silhouette_composite", &t.ink_b, &edge_params);
            }
            if edges_on {
                composite(&mut enc, "cadpole.crease_composite", &t.ink_a, &edge_params);
            }
            if !up.highlights.is_empty() {
                draw_ink(&mut enc, "cadpole.highlight", &t.ink_b, &t.ink_a, up.highlights.len() as u32, &highlight_buf);
                composite(&mut enc, "cadpole.highlight_composite", &t.ink_b, &highlight_params);
            }
        }

        // --- encode and read back -----------------------------------------
        {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cadpole.encode"),
                layout: &pipes.layouts.encode,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.display) }],
            });
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.encode"),
                color_attachments: &[Some(color_att(&t.out_view, wgpu::Color::TRANSPARENT))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipes.encode);
            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &bg, &[]);
            pass.draw(0..3, 0..1);
        }
        copy_to_buffer(&mut enc, &t.out, &t.readback, w, h, t.row_bytes);

        // --- id buffer ----------------------------------------------------
        if self.picking {
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("cadpole.ids"),
                    color_attachments: &[Some(color_att(
                        &t.pick_view,
                        wgpu::Color { r: NO_ITEM as f64, g: u32::MAX as f64, b: 0.0, a: 0.0 },
                    ))],
                    depth_stencil_attachment: Some(depth_att(&t.pick_depth)),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &globals_bg, &[]);
                if !up.opaque.is_empty() {
                    pass.set_pipeline(&pipes.pick_faces);
                    pass.set_vertex_buffer(0, opaque_buf.slice(..));
                    pass.draw(0..up.opaque.len() as u32, 0..1);
                }
                if !up.translucent.is_empty() {
                    pass.set_pipeline(&pipes.pick_faces);
                    pass.set_vertex_buffer(0, glass_buf.slice(..));
                    pass.draw(0..up.translucent.len() as u32, 0..1);
                }
                if !up.pick_edges.is_empty() {
                    pass.set_pipeline(&pipes.pick_edges);
                    pass.set_vertex_buffer(0, pick_edge_buf.slice(..));
                    pass.draw(0..6, 0..up.pick_edges.len() as u32);
                }
            }
            copy_to_buffer(&mut enc, &t.pick, &t.pick_readback, w, h, t.pick_row_bytes);
        }

        queue.submit([enc.finish()]);
        self.globals_bg = Some(globals_bg);
        self.gpu.flush()
    }

    fn read_image(&mut self, w: u32, h: u32) -> Result<Image, Error> {
        let t = self.targets.as_ref().expect("targets");
        let row = t.row_bytes as usize;
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        map_read(&self.gpu, &t.readback, |data| {
            for y in 0..h as usize {
                let src = y * row;
                let dst = y * (w as usize) * 4;
                rgba[dst..dst + (w as usize) * 4].copy_from_slice(&data[src..src + (w as usize) * 4]);
            }
        })?;
        Ok(Image { width: w, height: h, rgba })
    }

    fn read_pick(&mut self, w: u32, h: u32) -> Result<(), Error> {
        let t = self.targets.as_ref().expect("targets");
        let row = t.pick_row_bytes as usize;
        let mut out = vec![[NO_ITEM, u32::MAX]; (w as usize) * (h as usize)];
        map_read(&self.gpu, &t.pick_readback, |data| {
            for y in 0..h as usize {
                let src = y * row;
                let ids: &[u32] = bytemuck::cast_slice(&data[src..src + (w as usize) * 8]);
                for x in 0..w as usize {
                    out[y * w as usize + x] = [ids[x * 2], ids[x * 2 + 1]];
                }
            }
        })?;
        self.pick_data = out;
        self.pick_size = (w, h);
        Ok(())
    }

    /// The nearest non-empty id within `radius` pixels of `(x, y)` in the last
    /// frame. Edges win over faces at the same distance, which is what makes
    /// clicking a corner select the edge rather than one of the two faces.
    ///
    /// This is an id-buffer readback, and it replaces the CPU
    /// raycast the interim three.js viewer does in TypeScript.
    pub fn pick(&self, x: u32, y: u32, radius: u32) -> Option<Pick> {
        let (w, h) = self.pick_size;
        if w == 0 || self.pick_data.is_empty() {
            return None;
        }
        let r = radius.min(64) as i64;
        let mut best: Option<(u64, Pick)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (px, py) = (x as i64 + dx, y as i64 + dy);
                if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                    continue;
                }
                let [item, id] = self.pick_data[(py as usize) * (w as usize) + px as usize];
                if item == NO_ITEM {
                    continue;
                }
                let d2 = (dx * dx + dy * dy) as u64;
                let is_edge = id & PICK_EDGE != 0;
                // Edges sort before faces at equal distance.
                let key = d2 * 2 + u64::from(!is_edge);
                let hit = Pick {
                    item,
                    face: if is_edge { None } else { Some(id) },
                    edge: if is_edge { Some(id & !PICK_EDGE) } else { None },
                    distance: ((dx * dx + dy * dy) as f64).sqrt() as u32,
                };
                if best.as_ref().is_none_or(|(k, _)| key < *k) {
                    best = Some((key, hit));
                }
            }
        }
        best.map(|(_, p)| p)
    }
}

fn color_att(view: &wgpu::TextureView, clear: wgpu::Color) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
    }
}

fn load_att(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
    }
}

/// Reverse-Z: cleared to 0, which `view_z` reads back as "nothing here".
fn depth_att(view: &wgpu::TextureView) -> wgpu::RenderPassDepthStencilAttachment<'_> {
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
        stencil_ops: None,
    }
}

fn copy_to_buffer(
    enc: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    buffer: &wgpu::Buffer,
    w: u32,
    h: u32,
    row_bytes: u32,
) {
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

fn map_read(gpu: &Gpu, buffer: &wgpu::Buffer, mut f: impl FnMut(&[u8])) -> Result<(), Error> {
    let slice = buffer.slice(..);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        if r.is_err() {
            // The poll below reports the device error; this only needs to stop
            // the wait.
        }
        flag.store(true, std::sync::atomic::Ordering::Release);
    });
    gpu.flush()?;
    if !done.load(std::sync::atomic::Ordering::Acquire) {
        return Err(Error::Device("buffer mapping never completed".into()));
    }
    f(&slice.get_mapped_range());
    buffer.unmap();
    Ok(())
}

/// The frame uniform: the camera basis the CPU renderer's `View` derives, the
/// scene bounds every stage scales its constants by, and the switches the
/// shaders read instead of recompiling.
fn build_globals(up: &Upload, cam: &Camera, opts: &RenderOptions, samples: u32, w: u32, h: u32) -> Globals {
    let fwd = (cam.target - cam.eye).normalized();
    let right = fwd.cross(cam.up).normalized();
    let upv = right.cross(fwd);
    let focal = (h as f64 * 0.5) / (cam.fov_degrees.to_radians() * 0.5).tan();
    let dist = (cam.target - cam.eye).length();
    let ortho_scale = focal / dist.max(1e-9);
    let sc = up.scene;

    // View-space depths of the bounding box corners, which set the near plane
    // (and, in orthographic, the far plane too).
    let mut zmin = f64::MAX;
    let mut zmax = f64::MIN;
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { sc.lo[0] } else { sc.hi[0] } as f64,
            if i & 2 == 0 { sc.lo[1] } else { sc.hi[1] } as f64,
            if i & 4 == 0 { sc.lo[2] } else { sc.hi[2] } as f64,
        );
        let z = (c - cam.eye).dot(fwd);
        zmin = zmin.min(z);
        zmax = zmax.max(z);
    }
    let diag = sc.diag as f64;
    let (near, far) = if cam.orthographic {
        let pad = 0.05 * diag + 1e-3;
        (zmin - pad, (zmax + pad).max(zmin - pad + 1e-6))
    } else {
        let near = if zmin > 0.0 { (zmin * 0.5).max(diag * 1e-5) } else { diag * 1e-5 };
        (near.max(1e-9), (zmax + diag).max(1.0))
    };

    let ex = cam.eye.dot(right);
    let ey = cam.eye.dot(upv);
    let ez = cam.eye.dot(fwd);
    let mut m = [[0.0f32; 4]; 4];
    if cam.orthographic {
        let a = 2.0 * ortho_scale / w as f64;
        let b = 2.0 * ortho_scale / h as f64;
        let span = far - near;
        for (c, axis) in [right, upv, fwd].into_iter().enumerate() {
            let _ = axis;
            let r = [right.x, right.y, right.z][c];
            let u = [upv.x, upv.y, upv.z][c];
            let fv = [fwd.x, fwd.y, fwd.z][c];
            m[c] = [(a * r) as f32, (b * u) as f32, (-fv / span) as f32, 0.0];
        }
        m[3] = [(-a * ex) as f32, (-b * ey) as f32, ((far + ez) / span) as f32, 1.0];
    } else {
        let a = 2.0 * focal / w as f64;
        let b = 2.0 * focal / h as f64;
        for c in 0..3 {
            let r = [right.x, right.y, right.z][c];
            let u = [upv.x, upv.y, upv.z][c];
            let fv = [fwd.x, fwd.y, fwd.z][c];
            m[c] = [(a * r) as f32, (b * u) as f32, 0.0, fv as f32];
        }
        m[3] = [(-a * ex) as f32, (-b * ey) as f32, near as f32, (-ez) as f32];
    }

    let step = nice_step((sc.diag as f64 / 8.0).max(1e-9)) as f32;
    let hf = &up.heightfield;
    Globals {
        view_proj: m,
        eye: [cam.eye.x as f32, cam.eye.y as f32, cam.eye.z as f32, f32::from(u8::from(cam.orthographic))],
        right: [right.x as f32, right.y as f32, right.z as f32, focal as f32],
        up: [upv.x as f32, upv.y as f32, upv.z as f32, ortho_scale as f32],
        fwd: [fwd.x as f32, fwd.y as f32, fwd.z as f32, near as f32],
        viewport: [w as f32, h as f32, far as f32, opts.edge_crease_degrees.clamp(0.0, 180.0)],
        scene_lo: [sc.lo[0], sc.lo[1], sc.lo[2], sc.diag],
        scene_hi: [sc.hi[0], sc.hi[1], sc.hi[2], sc.ground_z],
        center: [sc.center[0], sc.center[1], sc.center[2], opts.exposure],
        background: [
            opts.background[0],
            opts.background[1],
            opts.background[2],
            opts.tonemap.index() as f32,
        ],
        grid: [step, step * 10.0, 0.8 * sc.diag, 2.8 * sc.diag],
        ao: [
            AO_RADIUS * sc.diag,
            AO_BIAS * sc.diag,
            AO_BLUR_DEPTH * sc.diag,
            f32::from(u8::from(opts.ambient_occlusion)),
        ],
        hf: [hf.origin[0], hf.origin[1], hf.cell[0], hf.cell[1]],
        hf2: [
            hf.size as f32,
            sc.ground_z,
            f32::from(u8::from(hf.enabled)),
            0.1 * sc.diag,
        ],
        counts: [samples, up.materials.len() as u32, 0, 0],
    }
}

/// `lines::nice_step`: 1, 2 or 5 times a power of ten.
fn nice_step(raw: f64) -> f64 {
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

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    0.0
}

/// Render one image on the GPU, opening and closing a device around it.
///
/// Convenience for one-shot callers (`cadpole render --backend gpu`); anything
/// that renders more than once should keep a [`Renderer`].
#[cfg(not(target_arch = "wasm32"))]
pub fn render(items: &[cadpole_core::render::Item<'_>], cam: &Camera, opts: &RenderOptions) -> Result<Image, Error> {
    let mut r = Renderer::new(RendererOptions { picking: false, ..RendererOptions::new() })?;
    r.render(items, cam, opts)
}

/// Whether this machine has any adapter at all, software included. Cheap
/// enough for a CLI to branch on and for a test to skip on.
#[cfg(not(target_arch = "wasm32"))]
pub fn is_available() -> bool {
    Renderer::new(RendererOptions::new()).is_ok()
}

#[cfg(test)]
mod testing {
    //! Scenes the tests render. Deliberately hand-built rather than borrowed
    //! from `cadpole_core::render::testing`, which is `pub(crate)` there: a box
    //! whose six faces have known ids is what makes the picking assertion a
    //! statement about a *known* face rather than about whatever came back.

    use cadpole_core::context::Material;
    use cadpole_core::geom::Vec3;
    use cadpole_core::tessellate::Mesh;

    /// An axis-aligned box as six quads, one topology face per side (0 = -x,
    /// 1 = +x, 2 = -y, 3 = +y, 4 = -z, 5 = +z), with its twelve edges as
    /// polylines and their dihedral angles (all 90 degrees).
    pub fn boxy(lo: Vec3, hi: Vec3) -> (Mesh, Vec<Vec<Vec3>>, Vec<f64>) {
        let c = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        };
        // (corner indices in winding order, outward normal) per face.
        let faces: [([usize; 4], Vec3); 6] = [
            ([0, 4, 6, 2], Vec3::new(-1.0, 0.0, 0.0)),
            ([1, 3, 7, 5], Vec3::new(1.0, 0.0, 0.0)),
            ([0, 1, 5, 4], Vec3::new(0.0, -1.0, 0.0)),
            ([2, 6, 7, 3], Vec3::new(0.0, 1.0, 0.0)),
            ([0, 2, 3, 1], Vec3::new(0.0, 0.0, -1.0)),
            ([4, 5, 7, 6], Vec3::new(0.0, 0.0, 1.0)),
        ];
        let mut mesh = Mesh::default();
        let mut edges: Vec<Vec<Vec3>> = Vec::new();
        for (fi, (quad, n)) in faces.iter().enumerate() {
            let base = (mesh.positions.len() / 3) as u32;
            for &k in quad {
                let p = c(k);
                mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
                mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
            }
            mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            mesh.face_ids.extend_from_slice(&[fi as u32, fi as u32]);
            for w in 0..4 {
                let (a, b) = (c(quad[w]), c(quad[(w + 1) % 4]));
                // Each edge is shared by two faces; keep one copy.
                let same = |e: &Vec<Vec3>| {
                    ((e[0] - a).length() < 1e-9 && (e[1] - b).length() < 1e-9)
                        || ((e[0] - b).length() < 1e-9 && (e[1] - a).length() < 1e-9)
                };
                if !edges.iter().any(same) {
                    edges.push(vec![a, b]);
                }
            }
        }
        let dihedrals = vec![90.0; edges.len()];
        (mesh, edges, dihedrals)
    }

    pub fn material(texture: &str, scale: f64) -> Material {
        Material { texture: texture.into(), texture_scale: scale, ..Material::default() }
    }
}
