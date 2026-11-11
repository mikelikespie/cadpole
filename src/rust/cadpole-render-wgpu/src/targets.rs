//! Render targets, reallocated when the image size or sample count changes.

use crate::pipelines::{CONTACT, DEPTH, HDR, IDS, INK, NORMAL, OUT, PICK};
use crate::scene::HF_SIZE;

pub struct Targets {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub g_normal: wgpu::TextureView,
    pub g_ids: wgpu::TextureView,
    pub g_depth: wgpu::TextureView,
    pub t_normal: wgpu::TextureView,
    pub t_ids: wgpu::TextureView,
    pub t_depth: wgpu::TextureView,
    pub ao_raw: wgpu::TextureView,
    pub ao_tmp: wgpu::TextureView,
    pub ao: wgpu::TextureView,
    pub display: wgpu::TextureView,
    pub ink_a: wgpu::TextureView,
    pub ink_b: wgpu::TextureView,
    pub out: wgpu::Texture,
    pub out_view: wgpu::TextureView,
    pub pick: wgpu::Texture,
    pub pick_view: wgpu::TextureView,
    pub pick_depth: wgpu::TextureView,
    /// Baked ground contact shadow, uploaded once per render.
    pub contact: wgpu::Texture,
    pub contact_view: wgpu::TextureView,
    /// Read back to build the `Image`.
    pub readback: wgpu::Buffer,
    /// Read back to answer `pick()`.
    pub pick_readback: wgpu::Buffer,
    pub row_bytes: u32,
    pub pick_row_bytes: u32,
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    w: u32,
    h: u32,
    samples: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Texture-to-buffer copies want rows that are a multiple of 256 bytes.
pub fn padded_row(bytes: u32) -> u32 {
    bytes.div_ceil(256) * 256
}

impl Targets {
    pub fn new(device: &wgpu::Device, width: u32, height: u32, samples: u32) -> Targets {
        let (w, h) = (width.max(1), height.max(1));
        let att = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let att_bind = att | wgpu::TextureUsages::TEXTURE_BINDING;
        // A view keeps its texture alive, so the handles the renderer does
        // not copy from or write to are not kept around.
        let view = |t: wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = padded_row(w * 4);
        let pick_row_bytes = padded_row(w * 8);
        let out = texture(device, "cadpole.out", w, h, 1, OUT, att | wgpu::TextureUsages::COPY_SRC);
        let out_view = out.create_view(&wgpu::TextureViewDescriptor::default());
        let pick = texture(device, "cadpole.pick", w, h, 1, PICK, att | wgpu::TextureUsages::COPY_SRC);
        let pick_view = pick.create_view(&wgpu::TextureViewDescriptor::default());
        let contact = texture(
            device,
            "cadpole.contact",
            HF_SIZE,
            HF_SIZE,
            1,
            CONTACT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let contact_view = contact.create_view(&wgpu::TextureViewDescriptor::default());
        Targets {
            width: w,
            height: h,
            samples,
            g_normal: view(texture(device, "cadpole.g_normal", w, h, samples, NORMAL, att_bind)),
            g_ids: view(texture(device, "cadpole.g_ids", w, h, samples, IDS, att_bind)),
            g_depth: view(texture(device, "cadpole.g_depth", w, h, samples, DEPTH, att_bind)),
            t_normal: view(texture(device, "cadpole.t_normal", w, h, samples, NORMAL, att_bind)),
            t_ids: view(texture(device, "cadpole.t_ids", w, h, samples, IDS, att_bind)),
            t_depth: view(texture(device, "cadpole.t_depth", w, h, samples, DEPTH, att_bind)),
            ao_raw: view(texture(device, "cadpole.ao_raw", w, h, 1, INK, att_bind)),
            ao_tmp: view(texture(device, "cadpole.ao_tmp", w, h, 1, INK, att_bind)),
            ao: view(texture(device, "cadpole.ao", w, h, 1, INK, att_bind)),
            display: view(texture(device, "cadpole.display", w, h, 1, HDR, att_bind)),
            ink_a: view(texture(device, "cadpole.ink_a", w, h, 1, INK, att_bind)),
            ink_b: view(texture(device, "cadpole.ink_b", w, h, 1, INK, att_bind)),
            out,
            out_view,
            pick,
            pick_view,
            pick_depth: view(texture(device, "cadpole.pick_depth", w, h, 1, DEPTH, att)),
            contact,
            contact_view,
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cadpole.readback"),
                size: (row_bytes * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            pick_readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cadpole.pick_readback"),
                size: (pick_row_bytes * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            row_bytes,
            pick_row_bytes,
        }
    }

    pub fn matches(&self, width: u32, height: u32, samples: u32) -> bool {
        self.width == width.max(1) && self.height == height.max(1) && self.samples == samples
    }
}
