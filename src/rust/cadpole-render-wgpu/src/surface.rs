//! Rendering into a real surface rather than an offscreen texture: the path
//! the Apple apps and the browser will drive (docs/apple-apps.md).
//!
//! Only the parts that can be exercised without a window are wired: creating a
//! [`Viewport`] from a raw window handle, configuring and reconfiguring it, and
//! the render-on-demand bookkeeping. `render()` composes the same passes as the
//! headless path and blits the display texture to the swapchain image, so a
//! platform shell has to provide a handle, a size and a "draw now" call and
//! nothing else.
//!
//! What each platform still owes, and why it is not here:
//!
//! * **iOS / macOS.** A `CAMetalLayer`-backed view, whose *view* pointer
//!   becomes `RawWindowHandle::UiKit`/`AppKit` (wgpu has no CoreAnimationLayer variant; the handle is the
//!   view). The view must outlive the surface, `drawableSize` drives
//!   [`Viewport::resize`], and backgrounding must drop the surface while
//!   keeping the device — which is [`Viewport::suspend`] here. Those are
//!   Swift-side lifetimes, so they belong with the `cadpole_viewport_*` C ABI in
//!   cadpole-ffi, not in this crate.
//! * **Browser.** `SurfaceTarget::Canvas`/`OffscreenCanvas` needs wgpu's
//!   `webgpu` feature, which needs js-sys 0.3.104 and so wasm-bindgen 0.2.127;
//!   the workspace is pinned to 0.2.121 by rules_rust_wasm_bindgen 0.74.0 (see
//!   this crate's Cargo.toml). Until that rules bump, [`SurfaceSource::Canvas`]
//!   is refused with a diagnostic rather than silently producing a different
//!   renderer.
//! * **Desktop.** A winit window would be a `SurfaceSource::RawHandle` like any
//!   other; a desktop viewer is kept out of this crate.

use crate::{Error, Renderer};

/// Where a viewport draws.
pub enum SurfaceSource {
    /// A platform window or view. `display` may be `None` on platforms that do
    /// not have one (Metal), and must otherwise match the instance's.
    ///
    /// # Safety
    ///
    /// Both handles must be valid and must outlive the [`Viewport`]. On Apple
    /// platforms that means the view is destroyed only after
    /// `cadpole_viewport_destroy` has returned.
    RawHandle {
        display: Option<wgpu::rwh::RawDisplayHandle>,
        window: wgpu::rwh::RawWindowHandle,
    },
    /// An HTML canvas, once the wasm arm of this crate exists.
    Canvas,
    /// A canvas transferred to a worker (`transferControlToOffscreen`), the
    /// preferred configuration.
    OffscreenCanvas,
}

/// The render-on-demand bookkeeping, kept separate from the surface so it can
/// be tested without one.
///
/// SwiftUI does not drive frames: the controller pushes
/// state, marks the viewport dirty, and the display link draws until the scene
/// is quiescent and then pauses. Backgrounding drops the surface and keeps the
/// device, which is `suspended` here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    dirty: bool,
    suspended: bool,
}

impl Demand {
    /// A fresh viewport owes one frame.
    pub fn new() -> Demand {
        Demand { dirty: true, suspended: false }
    }

    pub fn set_needs_render(&mut self) {
        self.dirty = true;
    }

    /// Whether the display link should draw. A suspended viewport never does,
    /// however dirty it is.
    pub fn needs_render(&self) -> bool {
        self.dirty && !self.suspended
    }

    /// A frame reached the surface. Ignored while suspended: there is no
    /// surface to have drawn into, so the frame is still owed.
    pub fn drawn(&mut self) {
        if !self.suspended {
            self.dirty = false;
        }
    }

    /// `scenePhase == .background`, or an occluded window: no surface, and one
    /// frame owed when it comes back.
    pub fn suspend(&mut self) {
        self.suspended = true;
        self.dirty = true;
    }

    /// Back to the foreground. The swapchain is new, so a frame is owed
    /// whatever was pending before.
    pub fn resume(&mut self) {
        self.suspended = false;
        self.dirty = true;
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }
}

/// A configured surface plus the render-on-demand state around it.
pub struct Viewport {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// The format the frame is *viewed* through: never the sRGB variant, see
    /// `fs_blit`.
    view_format: wgpu::TextureFormat,
    blit: Option<wgpu::RenderPipeline>,
    demand: Demand,
}

impl Viewport {
    /// Create a surface for `source` and configure it at `width` x `height`.
    ///
    /// # Safety
    ///
    /// The handles in `source` must be valid and outlive the returned
    /// viewport.
    pub unsafe fn new(renderer: &Renderer, source: SurfaceSource, width: u32, height: u32) -> Result<Viewport, Error> {
        let target = match source {
            SurfaceSource::RawHandle { display, window } => wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: display,
                raw_window_handle: window,
            },
            SurfaceSource::Canvas | SurfaceSource::OffscreenCanvas => {
                return Err(Error::Unsupported(
                    "the canvas surface path needs wgpu's `webgpu` feature, which is blocked on \
                     wasm-bindgen 0.2.127 (see cadpole-render-wgpu/Cargo.toml)",
                ));
            }
        };
        let surface = unsafe { renderer.instance().create_surface_unsafe(target) }
            .map_err(|e| Error::Surface(e.to_string()))?;
        let caps = surface.get_capabilities(renderer.adapter_handle());
        if caps.formats.is_empty() {
            return Err(Error::Surface("adapter cannot present to this surface".into()));
        }
        // sRGB, because the renderer already tonemaps to display-linear and resolves
        // with a dither: the swapchain must apply the transfer function and
        // nothing else, or the app would be correcting twice.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb || *f == wgpu::TextureFormat::Rgba8UnormSrgb)
            .unwrap_or(caps.formats[0]);
        let view_format = format.remove_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: caps.present_modes.first().copied().unwrap_or(wgpu::PresentMode::Fifo),
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format { vec![] } else { vec![view_format] },
        };
        let vp = Viewport { surface, config, view_format, blit: None, demand: Demand::new() };
        vp.configure(renderer);
        Ok(vp)
    }

    fn configure(&self, renderer: &Renderer) {
        // Never configure a zero-sized surface: Stage Manager and split-view
        // transitions produce 0x0 bounds, which wgpu treats as a validation
        // failure.
        if self.config.width == 0 || self.config.height == 0 {
            return;
        }
        self.surface.configure(renderer.device(), &self.config);
    }

    /// Reconfigure after a `drawableSize` change. Never call mid-frame.
    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) {
        if width == 0 || height == 0 {
            // Keep the old configuration; the platform will resize again.
            self.demand.set_needs_render();
            return;
        }
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.demand.set_needs_render();
        if !self.demand.is_suspended() {
            self.configure(renderer);
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Mark the viewport as needing a frame. The platform's display link draws
    /// while this is true and pauses when it is not.
    pub fn set_needs_render(&mut self) {
        self.demand.set_needs_render();
    }

    pub fn needs_render(&self) -> bool {
        self.demand.needs_render()
    }

    /// Drop the swapchain, keeping the device: `scenePhase == .background`.
    pub fn suspend(&mut self) {
        self.demand.suspend();
    }

    /// Recreate the swapchain on returning to the foreground.
    pub fn resume(&mut self, renderer: &Renderer) {
        self.demand.resume();
        self.configure(renderer);
    }

    /// Draw one frame into the surface. Returns `Ok(false)` when there was no
    /// drawable -- a skipped frame, not an error.
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        items: &[cadpole_core::render::Item<'_>],
        cam: &cadpole_core::render::Camera,
        opts: &cadpole_core::render::RenderOptions,
    ) -> Result<bool, Error> {
        if self.demand.is_suspended() {
            return Ok(false);
        }
        use wgpu::CurrentSurfaceTexture as Cur;
        let frame = match self.surface.get_current_texture() {
            Cur::Success(f) | Cur::Suboptimal(f) => f,
            Cur::Outdated | Cur::Lost => {
                // Surface loss is recoverable: reconfigure and let the caller
                // ask for another frame.
                self.configure(renderer);
                return Ok(false);
            }
            // "No drawable available" is a skipped frame, not an error.
            Cur::Timeout | Cur::Occluded => return Ok(false),
            Cur::Validation => return Err(Error::Surface("surface validation error".into())),
        };
        // The offscreen path renders into an `Rgba8Unorm` texture that already
        // carries the sRGB *encoding* from `fs_encode`; blitting it into an
        // sRGB swapchain image would apply the transfer twice, so the copy is a
        // straight texel copy rather than a draw.
        let opts = cadpole_core::render::RenderOptions {
            width: self.config.width,
            height: self.config.height,
            ..opts.clone()
        };
        renderer.render_to_texture(items, cam, &opts)?;
        if self.blit.is_none() {
            self.blit = Some(blit_pipeline(renderer, self.view_format)?);
        }
        let src = renderer.output_texture().expect("output texture").create_view(&Default::default());
        let bind = renderer.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadpole.blit"),
            layout: renderer.blit_layout().expect("blit layout"),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) }],
        });
        let dst = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut enc = renderer
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cadpole.present") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cadpole.present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.blit.as_ref().expect("blit pipeline"));
            pass.set_bind_group(0, renderer.globals_bind_group(), &[]);
            pass.set_bind_group(1, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        renderer.queue().submit([enc.finish()]);
        frame.present();
        self.demand.drawn();
        Ok(true)
    }
}

/// The blit pipeline for one surface format. Built lazily, because the
/// renderer's own pipelines are built for the offscreen format and a surface
/// may want any of several.
pub(crate) fn blit_pipeline(renderer: &Renderer, format: wgpu::TextureFormat) -> Result<wgpu::RenderPipeline, Error> {
    let device = renderer.device();
    let layouts = renderer.pipeline_layouts().ok_or(Error::Unsupported("render at least one frame first"))?;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("cadpole.blit"),
        source: wgpu::ShaderSource::Wgsl(crate::pipelines::module_source("encode", renderer.multisampled()).into()),
    });
    Ok(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cadpole.blit"),
        layout: Some(layouts),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_blit"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    }))
}
