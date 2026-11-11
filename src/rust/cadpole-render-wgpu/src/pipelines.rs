//! Shader modules, bind group layouts and pipelines: everything that depends
//! on the device but not on the image size or the scene, built once per
//! [`crate::Renderer`].

use wgpu::util::DeviceExt;

use crate::scene::{Seg, Vertex};

pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const NORMAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// item, face low, face high, flags. 16-bit uint is the widest integer format
/// WebGPU guarantees can be multisampled.
pub const IDS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Uint;
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const INK: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
pub const OUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const PICK: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
pub const CONTACT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

const COMMON: &str = include_str!("shaders/common.wgsl");
const TEXTURES: &str = include_str!("shaders/textures.wgsl");
const SHADING: &str = include_str!("shaders/shading.wgsl");
const GEOM: &str = include_str!("shaders/geom.wgsl");
const SSAO: &str = include_str!("shaders/ssao.wgsl");
const SHADE: &str = include_str!("shaders/shade.wgsl");
const OVERLAY: &str = include_str!("shaders/overlay.wgsl");
const ENCODE: &str = include_str!("shaders/encode.wgsl");

/// One module's source by name, for callers that build a pipeline of their own
/// (the surface blit).
pub fn module_source(name: &str, multisampled: bool) -> String {
    modules(multisampled).into_iter().find(|(n, _)| *n == name).map(|(_, s)| s).unwrap_or_default()
}

/// Every WGSL module the renderer compiles, as (name, source). Concatenation
/// is the whole of the include mechanism: WGSL has none, and the CPU
/// rasterizer's stages are shared the same way.
///
/// `multisampled` is false when the image is rendered at one sample per pixel
/// (`--samples 1`, or above 16 Mpx, where the CPU renderer also drops to one).
/// A `texture_multisampled_2d` binding is invalid for a single-sample texture,
/// and the *calls* are identical either way -- `textureLoad(t, coord, i)` takes
/// a sample index on a multisampled texture and a mip level on a plain one, and
/// the index is always 0 there -- so swapping the declarations is the whole of
/// the difference.
pub fn modules(multisampled: bool) -> Vec<(&'static str, String)> {
    let fix = |s: String| {
        if multisampled {
            s
        } else {
            s.replace("texture_depth_multisampled_2d", "texture_depth_2d")
                .replace("texture_multisampled_2d", "texture_2d")
        }
    };
    vec![
        ("geom", fix([COMMON, GEOM].concat())),
        ("ssao", fix([COMMON, SSAO].concat())),
        ("shade", fix([COMMON, TEXTURES, SHADING, SHADE].concat())),
        ("overlay", fix([COMMON, OVERLAY].concat())),
        ("encode", fix([COMMON, ENCODE].concat())),
    ]
}

fn entry(binding: u32, visibility: wgpu::ShaderStages, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None }
}

fn tex_ms(sample: wgpu::TextureSampleType, multisampled: bool) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: sample,
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled,
    }
}

fn tex_1x(sample: wgpu::TextureSampleType) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: sample,
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

const UNFILTERABLE: wgpu::TextureSampleType = wgpu::TextureSampleType::Float { filterable: false };
const DEPTH_SAMPLE: wgpu::TextureSampleType = wgpu::TextureSampleType::Depth;
const UINT: wgpu::TextureSampleType = wgpu::TextureSampleType::Uint;

pub struct Layouts {
    pub globals: wgpu::BindGroupLayout,
    pub ssao: wgpu::BindGroupLayout,
    pub shade: wgpu::BindGroupLayout,
    pub overlay: wgpu::BindGroupLayout,
    pub encode: wgpu::BindGroupLayout,
}

impl Layouts {
    fn new(device: &wgpu::Device, ms: bool) -> Layouts {
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let f = wgpu::ShaderStages::FRAGMENT;
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadpole.globals"),
            entries: &[
                entry(
                    0,
                    vf,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    f,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let ssao = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadpole.ssao"),
            entries: &[
                entry(0, f, tex_ms(DEPTH_SAMPLE, ms)),
                entry(1, f, tex_ms(UNFILTERABLE, ms)),
                entry(2, f, tex_ms(UINT, ms)),
                entry(3, f, tex_1x(UNFILTERABLE)),
            ],
        });
        let shade = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadpole.shade"),
            entries: &[
                entry(0, f, tex_ms(DEPTH_SAMPLE, ms)),
                entry(1, f, tex_ms(UNFILTERABLE, ms)),
                entry(2, f, tex_ms(UINT, ms)),
                entry(3, f, tex_1x(UNFILTERABLE)),
                entry(4, f, tex_1x(UNFILTERABLE)),
                entry(5, f, tex_ms(DEPTH_SAMPLE, ms)),
                entry(6, f, tex_ms(UNFILTERABLE, ms)),
                entry(7, f, tex_ms(UINT, ms)),
            ],
        });
        let overlay = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadpole.overlay"),
            entries: &[
                entry(0, f, tex_ms(DEPTH_SAMPLE, ms)),
                entry(1, f, tex_ms(UINT, ms)),
                entry(2, f, tex_ms(UNFILTERABLE, ms)),
                entry(3, f, tex_1x(UNFILTERABLE)),
                entry(
                    4,
                    f,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let encode = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadpole.encode"),
            entries: &[entry(0, f, tex_1x(UNFILTERABLE))],
        });
        Layouts { globals, ssao, shade, overlay, encode }
    }
}

const VERTEX_ATTRS: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 24, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 28, shader_location: 3 },
];

/// The overlay's view of a segment: ends, half width, alpha, flags.
const SEG_ATTRS: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 28, shader_location: 3 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 32, shader_location: 4 },
];

/// The id pass's view of the same buffer: ends, half width, item and pick id.
const PICK_SEG_ATTRS: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 36, shader_location: 5 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 40, shader_location: 6 },
];

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRS,
    }
}

fn seg_layout(attrs: &'static [wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Seg>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: attrs,
    }
}

/// Reverse-Z: a fragment is visible when its depth is *greater* than what is
/// already there, and the buffer is cleared to 0.
fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::Greater),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// `dst * (1 - src.a) + src`: the premultiplied source the overlay passes emit,
/// which reproduces the CPU's `lerp3(display, ink, coverage)`.
fn over_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// Ink layers combine with `max`, so a polyline does not darken at its joins.
fn max_blend() -> wgpu::BlendState {
    let c = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    };
    wgpu::BlendState { color: c, alpha: c }
}

pub struct Pipelines {
    pub layouts: Layouts,
    /// Opaque and translucent G-buffer passes, indexed by sample count.
    pub geom: wgpu::RenderPipeline,
    pub glass: wgpu::RenderPipeline,
    pub ssao: wgpu::RenderPipeline,
    pub blur_x: wgpu::RenderPipeline,
    pub blur_y: wgpu::RenderPipeline,
    pub shade: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub edge: wgpu::RenderPipeline,
    pub silhouette: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
    pub encode: wgpu::RenderPipeline,
    pub pick_faces: wgpu::RenderPipeline,
    pub pick_edges: wgpu::RenderPipeline,
    /// Kept so the surface path can build a blit pipeline for its own format
    /// against the same bind groups.
    pub encode_layout: wgpu::PipelineLayout,
    /// Sample count these pipelines were built for.
    pub samples: u32,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, samples: u32) -> Pipelines {
        let multisampled = samples > 1;
        let layouts = Layouts::new(device, multisampled);
        let src = modules(multisampled);
        let module = |name: &str| {
            let (_, code) = src.iter().find(|(n, _)| *n == name).expect("shader module");
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(code.as_str().into()),
            })
        };
        let m_geom = module("geom");
        let m_ssao = module("ssao");
        let m_shade = module("shade");
        let m_overlay = module("overlay");
        let m_encode = module("encode");

        let pl = |label: &str, extra: &[&wgpu::BindGroupLayout]| {
            let mut sets: Vec<Option<&wgpu::BindGroupLayout>> = vec![Some(&layouts.globals)];
            sets.extend(extra.iter().map(|l| Some(*l)));
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &sets,
                immediate_size: 0,
            })
        };
        let ms = |n: u32| wgpu::MultisampleState { count: n, mask: !0, alpha_to_coverage_enabled: false };
        let tri = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // No culling: the CPU rasterizer draws both faces and lets depth
            // decide, which is what an open or inside-out mesh needs.
            cull_mode: None,
            ..Default::default()
        };

        let geom_layout = pl("cadpole.geom", &[]);
        let gbuf_targets = [
            Some(wgpu::ColorTargetState { format: NORMAL, blend: None, write_mask: wgpu::ColorWrites::ALL }),
            Some(wgpu::ColorTargetState { format: IDS, blend: None, write_mask: wgpu::ColorWrites::ALL }),
        ];
        let geom = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadpole.geom"),
            layout: Some(&geom_layout),
            vertex: wgpu::VertexState {
                module: &m_geom,
                entry_point: Some("vs_geom"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout()],
            },
            primitive: tri,
            depth_stencil: Some(depth_state(true)),
            multisample: ms(samples),
            fragment: Some(wgpu::FragmentState {
                module: &m_geom,
                entry_point: Some("fs_geom"),
                compilation_options: Default::default(),
                targets: &gbuf_targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        // Same shaders, its own depth buffer: the nearest translucent fragment
        // wins, and the shading pass rejects any that is behind the opaque
        // surface.
        let glass = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadpole.glass"),
            layout: Some(&geom_layout),
            vertex: wgpu::VertexState {
                module: &m_geom,
                entry_point: Some("vs_geom"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout()],
            },
            primitive: tri,
            depth_stencil: Some(depth_state(true)),
            multisample: ms(samples),
            fragment: Some(wgpu::FragmentState {
                module: &m_geom,
                entry_point: Some("fs_geom"),
                compilation_options: Default::default(),
                targets: &gbuf_targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let fullscreen = |label: &str,
                          module: &wgpu::ShaderModule,
                          layout: &wgpu::PipelineLayout,
                          entry: &str,
                          format: wgpu::TextureFormat,
                          blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: ms(1),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let ssao_layout = pl("cadpole.ssao", &[&layouts.ssao]);
        let ssao = fullscreen("cadpole.ssao", &m_ssao, &ssao_layout, "fs_ssao", INK, None);
        let blur_x = fullscreen("cadpole.blur_x", &m_ssao, &ssao_layout, "fs_blur_x", INK, None);
        let blur_y = fullscreen("cadpole.blur_y", &m_ssao, &ssao_layout, "fs_blur_y", INK, None);

        let shade_layout = pl("cadpole.shade", &[&layouts.shade]);
        let shade = fullscreen("cadpole.shade", &m_shade, &shade_layout, "fs_shade", HDR, None);

        let overlay_layout = pl("cadpole.overlay", &[&layouts.overlay]);
        let grid = fullscreen("cadpole.grid", &m_overlay, &overlay_layout, "fs_grid", HDR, Some(over_blend()));
        let silhouette = fullscreen("cadpole.silhouette", &m_overlay, &overlay_layout, "fs_silhouette", INK, Some(max_blend()));
        let composite = fullscreen("cadpole.composite", &m_overlay, &overlay_layout, "fs_composite", HDR, Some(over_blend()));

        let edge = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadpole.edge"),
            layout: Some(&overlay_layout),
            vertex: wgpu::VertexState {
                module: &m_overlay,
                entry_point: Some("vs_edge"),
                compilation_options: Default::default(),
                buffers: &[seg_layout(&SEG_ATTRS)],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: ms(1),
            fragment: Some(wgpu::FragmentState {
                module: &m_overlay,
                entry_point: Some("fs_edge"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: INK,
                    blend: Some(max_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let encode_layout = pl("cadpole.encode", &[&layouts.encode]);
        let encode = fullscreen("cadpole.encode", &m_encode, &encode_layout, "fs_encode", OUT, None);

        let pick_targets = [Some(wgpu::ColorTargetState {
            format: PICK,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pick_faces = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadpole.pick_faces"),
            layout: Some(&geom_layout),
            vertex: wgpu::VertexState {
                module: &m_geom,
                entry_point: Some("vs_geom"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout()],
            },
            primitive: tri,
            depth_stencil: Some(depth_state(true)),
            multisample: ms(1),
            fragment: Some(wgpu::FragmentState {
                module: &m_geom,
                entry_point: Some("fs_ids"),
                compilation_options: Default::default(),
                targets: &pick_targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let pick_edges = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadpole.pick_edges"),
            layout: Some(&geom_layout),
            vertex: wgpu::VertexState {
                module: &m_geom,
                entry_point: Some("vs_edge_id"),
                compilation_options: Default::default(),
                buffers: &[seg_layout(&PICK_SEG_ATTRS)],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            // Depth-tested against the faces so a hidden edge does not win,
            // but never written: overlapping edges must all be able to draw.
            depth_stencil: Some(depth_state(false)),
            multisample: ms(1),
            fragment: Some(wgpu::FragmentState {
                module: &m_geom,
                entry_point: Some("fs_edge_id"),
                compilation_options: Default::default(),
                targets: &pick_targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        Pipelines {
            layouts,
            geom,
            glass,
            ssao,
            blur_x,
            blur_y,
            shade,
            grid,
            edge,
            silhouette,
            composite,
            encode,
            pick_faces,
            pick_edges,
            encode_layout,
            samples,
        }
    }
}

/// A buffer holding `data`, with the given usage.
pub fn buffer<T: bytemuck::Pod>(device: &wgpu::Device, label: &str, data: &[T], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let bytes = bytemuck::cast_slice(data);
    // wgpu refuses a zero-sized buffer; one element of padding costs nothing
    // and keeps every "no edges in this scene" path free of special cases.
    let padding = [0u8; 64];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: if bytes.is_empty() { &padding } else { bytes },
        usage,
    })
}
