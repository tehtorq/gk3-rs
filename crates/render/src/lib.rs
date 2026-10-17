//! GPU renderer for GK3 rooms, built on wgpu.
//!
//! The renderer draws into any texture view, so the same code serves a window
//! and an offscreen image. wgpu loads the GPU driver (Vulkan, GL, Metal or
//! D3D12) at run time; nothing graphical is linked into the executable.

pub mod mesh;
pub mod model;
pub mod overlay;

use gk3_formats::image::Image;
use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

pub use mesh::{SceneMesh, SceneSource, Sky};
pub use model::{GpuModel, ModelLight, ModelVertex, SubmeshDesc};

/// A picture uploaded once for painting on models (see
/// `Renderer::model_texture`).
#[derive(Clone)]
pub struct ModelTexture(wgpu::TextureView);
pub use overlay::{Overlay, OverlayTexture, Quad, WHITE};

/// Colour format for offscreen rendering. Not sRGB: GK3 multiplied lightmaps
/// in gamma space, and doing it in linear space would change the look.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A first-person camera in GK3's left-handed, Y-up world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub position: Vec3,
    /// Heading in radians; zero looks along +Z.
    pub yaw: f32,
    /// Radians; positive looks down, as in GK3's camera angles.
    pub pitch: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
}

impl Camera {
    /// GK3's own convention, matching G-Engine: rotate about Y by the
    /// heading, then about X by the pitch.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation() * Vec3::X
    }

    fn projection(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_lh(self.fov_y, aspect, 4.0, 40_000.0)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * Mat4::look_to_lh(self.position, self.forward(), Vec3::Y)
    }
}

/// Where a frame is drawn.
pub struct Target<'a> {
    pub color: &'a wgpu::TextureView,
    pub depth: &'a FrameBuffers,
    pub size: (u32, u32),
}

/// What a frame is drawn into before it reaches its target: a depth buffer
/// and, with multisampling, the colour buffer the samples go into, which
/// the end of the frame resolves into the target. Made for one size by
/// `Renderer::frame_buffers`, and made again when that changes.
pub struct FrameBuffers {
    depth: wgpu::TextureView,
    multisampled: Option<wgpu::TextureView>,
}

/// Anisotropic filtering's samples, at most: 16 is what GPUs offer.
pub const MAX_ANISOTROPY: u16 = 16;

/// Samples a pixel when multisampling: 4 is what every GPU offers.
pub const MSAA_SAMPLES: u32 = 4;

/// What the scene shader shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadeMode {
    #[default]
    Lit,
    TexturesOnly,
    LightmapsOnly,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    sky_inverse: [[f32; 4]; 4],
    atlas_size: [f32; 2],
    mode: u32,
    _pad: u32,
}

/// How room and model textures are filtered: the original's advanced
/// graphics options. Without mipmaps, far textures are sampled at full size
/// (sharper, and shimmering); without trilinear filtering, the nearest
/// mipmap is used rather than a mix of two. With both, sampling is also
/// anisotropic (up to the renderer's limit), which keeps floors and walls
/// seen at a slant sharp rather than blurred to their narrow side's level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filtering {
    pub mipmaps: bool,
    pub trilinear: bool,
}

impl Default for Filtering {
    fn default() -> Self {
        Self {
            mipmaps: true,
            trilinear: true,
        }
    }
}

fn world_sampler(device: &wgpu::Device, filtering: Filtering, anisotropy: u16) -> wgpu::Sampler {
    // Anisotropy needs every filter linear.
    let linear = filtering.mipmaps && filtering.trilinear;
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("repeat"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: if filtering.trilinear {
            wgpu::MipmapFilterMode::Linear
        } else {
            wgpu::MipmapFilterMode::Nearest
        },
        lod_max_clamp: if filtering.mipmaps { 32.0 } else { 0.0 },
        anisotropy_clamp: if linear { anisotropy.clamp(1, 16) } else { 1 },
        ..Default::default()
    })
}

/// Opens a GPU with no window, for offscreen rendering and tools.
pub fn headless() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| format!("no usable GPU: {e}"))?;
    let name = adapter.get_info().name;
    let (device, queue) = request_device(&adapter)?;
    Ok((device, queue, name))
}

/// How many samples a pixel a window on this adapter should draw: smoothed
/// edges (`MSAA_SAMPLES`) on a real GPU that can, one on a software
/// renderer, where it would cost too much.
pub fn window_samples(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> u32 {
    let can = |f| {
        adapter
            .get_texture_format_features(f)
            .flags
            .sample_count_supported(MSAA_SAMPLES)
    };
    let software = adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    if !software && can(format) && can(DEPTH_FORMAT) {
        MSAA_SAMPLES
    } else {
        1
    }
}

pub fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("gk3"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|e| format!("could not open the GPU: {e}"))
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    scene_pipeline: wgpu::RenderPipeline,
    decal_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    /// The same, not drawing back faces: for characters.
    one_sided_pipeline: wgpu::RenderPipeline,
    /// Blob shadows: multiplied onto what is already drawn.
    shadow_pipeline: wgpu::RenderPipeline,
    light_layout: wgpu::BindGroupLayout,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_textures: Vec<(wgpu::BindGroup, (u32, u32))>,
    /// The overlay textures themselves, for redrawing them in place.
    overlay_images: Vec<wgpu::Texture>,
    globals_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sky_layout: wgpu::BindGroupLayout,
    repeat_sampler: wgpu::Sampler,
    clamp_sampler: wgpu::Sampler,
    /// The colour format the pipelines draw in; offscreen pictures use it
    /// too, or the pipelines would not fit them.
    color_format: wgpu::TextureFormat,
    /// Samples a pixel: 1, or `MSAA_SAMPLES` to smooth edges.
    samples: u32,
    /// The GPU's name, for the graphics options to show.
    pub adapter_name: String,
    /// The most anisotropic filtering to use: `MAX_ANISOTROPY`, or 1 on a
    /// software renderer, where each extra sample costs.
    pub max_anisotropy: u16,
}

/// A room on the GPU.
pub struct GpuScene {
    vertices: wgpu::Buffer,
    batches: Vec<mesh::Batch>,
    /// Each texture and its binding, which a change of filtering remakes.
    textures: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    sky: Option<(wgpu::BindGroup, f32)>,
    atlas_size: [f32; 2],
    pub bounds: ([f32; 3], [f32; 3]),
}

impl Renderer {
    /// A renderer drawing one sample a pixel.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        Self::with_samples(device, queue, color_format, 1)
    }

    /// A renderer drawing `samples` samples a pixel (1 or `MSAA_SAMPLES`),
    /// which smooths the edges of polygons.
    pub fn with_samples(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let multisample = wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        };
        let tex_entry = |binding, dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1, wgpu::TextureViewDimension::D2),
                sampler_entry(2),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("diffuse"),
            entries: &[
                tex_entry(0, wgpu::TextureViewDimension::D2),
                sampler_entry(1),
            ],
        });
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[
                tex_entry(0, wgpu::TextureViewDimension::Cube),
                sampler_entry(1),
            ],
        });

        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });

        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&globals_layout), Some(&sky_layout)],
            immediate_size: 0,
        });

        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32
        ];
        let target = [Some(wgpu::ColorTargetState {
            format: color_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let scene_pipeline_with = |label, blend: Option<wgpu::BlendState>, depth_write: bool, bias: wgpu::DepthBiasState| {
            let target = [Some(wgpu::ColorTargetState {
                format: color_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<mesh::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            // The room's polygons face one way, as G-Engine draws them:
            // clockwise is the front and the back is not drawn. Seen from
            // behind (a camera outside a wall), a wall is not there.
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias,
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        })
        };
        let scene_pipeline = scene_pipeline_with("scene", None, true, Default::default());
        // Shadows painted on surfaces: multiplied onto what is drawn, not
        // written to depth, and drawn a little nearer, as the original
        // (0x5938e0: blend zero and source colour, z bias 2).
        let decal_pipeline = scene_pipeline_with(
            "scene decals",
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::Src,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            false,
            wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 },
        );

        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });

        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model light"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let model_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()),
        });
        let model_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&texture_layout),
                Some(&light_layout),
            ],
            immediate_size: 0,
        });
        let model_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Unorm8x4
        ];
        // Characters are drawn one-sided as G-Engine draws models (so a
        // camera inside a head sees through it, not the face from behind);
        // props are drawn two-sided, as their leaf cards face either way.
        let model_pipeline_with = |label, cull_mode| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&model_layout),
            vertex: wgpu::VertexState {
                module: &model_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ModelVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &model_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &model_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });
        let model_pipeline = model_pipeline_with("model", None);
        let one_sided_pipeline = model_pipeline_with("one-sided model", Some(wgpu::Face::Back));

        // G-Engine draws an actor's blob shadow as a lightmap (times 2.1) on
        // a white quad, multiplied onto the floor.
        let shadow_target = [Some(wgpu::ColorTargetState {
            format: color_format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),
            write_mask: wgpu::ColorWrites::COLOR,
        })];
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&model_layout),
            vertex: wgpu::VertexState {
                module: &model_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ModelVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &model_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &model_shader,
                entry_point: Some("fs_shadow"),
                compilation_options: Default::default(),
                targets: &shadow_target,
            }),
            multiview_mask: None,
            cache: None,
        });

        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let overlay_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&texture_layout)],
            immediate_size: 0,
        });
        let overlay_attributes =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32x4];
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&overlay_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<overlay::OverlayVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &overlay_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            // On top of everything, whatever the depth says.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let repeat_sampler = world_sampler(&device, Filtering::default(), MAX_ANISOTROPY);
        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let mut renderer = Self {
            device,
            queue,
            scene_pipeline,
            decal_pipeline,
            sky_pipeline,
            model_pipeline,
            one_sided_pipeline,
            shadow_pipeline,
            light_layout,
            overlay_pipeline,
            overlay_textures: Vec::new(),
            overlay_images: Vec::new(),
            globals_layout,
            texture_layout,
            sky_layout,
            repeat_sampler,
            clamp_sampler,
            color_format,
            samples,
            adapter_name: String::new(),
            max_anisotropy: MAX_ANISOTROPY,
        };
        renderer.add_overlay_texture(&Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        renderer
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Uploads a room.
    pub fn upload(&self, mesh: &SceneMesh) -> GpuScene {
        let device = &self.device;
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene vertices"),
            contents: if mesh.vertices.is_empty() {
                &[0u8; 64]
            } else {
                bytemuck::cast_slice(&mesh.vertices)
            },
            usage: wgpu::BufferUsages::VERTEX,
        });

        let textures = mesh
            .textures
            .iter()
            .map(|image| {
                let view = self.texture(&color_key(image), true);
                let group = self.world_group(&view);
                (view, group)
            })
            .collect();

        let atlas = self.texture(&mesh.lightmap_atlas, false);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &self.globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.clamp_sampler),
                },
            ],
        });

        let sky = mesh.sky.as_ref().map(|sky| {
            let view = self.cube_texture(&sky.faces);
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky"),
                layout: &self.sky_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.clamp_sampler),
                    },
                ],
            });
            (group, sky.azimuth)
        });

        GpuScene {
            vertices,
            batches: mesh.batches.clone(),
            textures,
            globals,
            globals_group,
            sky,
            atlas_size: [
                mesh.lightmap_atlas.width as f32,
                mesh.lightmap_atlas.height as f32,
            ],
            bounds: mesh.bounds,
        }
    }

    /// Makes an image available to the overlay.
    pub fn add_overlay_texture(&mut self, image: &Image) -> OverlayTexture {
        let (texture, group) = self.overlay_group(image);
        self.overlay_textures
            .push((group, (image.width, image.height)));
        self.overlay_images.push(texture);
        OverlayTexture(self.overlay_textures.len() - 1)
    }

    /// Redraws an overlay texture with a new picture, such as the next frame
    /// of a movie; in place when the size is the same.
    pub fn update_overlay_texture(&mut self, texture: OverlayTexture, image: &Image) {
        let Some(old) = self.overlay_images.get(texture.0) else {
            return;
        };
        if (old.width(), old.height()) == (image.width, image.height) {
            self.write_layer(old, image, 0, 0);
            return;
        }
        let (new, group) = self.overlay_group(image);
        self.overlay_textures[texture.0] = (group, (image.width, image.height));
        self.overlay_images[texture.0] = new;
    }

    fn overlay_group(&self, image: &Image) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = self.texture_object(image);
        let view = texture.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.clamp_sampler),
                },
            ],
        });
        (texture, group)
    }

    /// Uploads a model's textures and triangles. Its vertices are supplied
    /// with [`Renderer::update_model`], in world space, whenever it moves.
    pub fn upload_model(&self, submeshes: &[SubmeshDesc]) -> GpuModel {
        let device = &self.device;
        let mut indices: Vec<u16> = Vec::new();
        let mut draws = Vec::new();
        let mut base_vertex = 0i32;
        let mut views = Vec::new();
        for sub in submeshes {
            let view = self.texture(&color_key(&sub.texture), true);
            let group = self.world_group(&view);
            views.push(view);
            let first = indices.len() as u32;
            indices.extend(sub.triangles.iter().flatten());
            draws.push((group, first, indices.len() as u32 - first, base_vertex));
            base_vertex += sub.vertex_count as i32;
        }
        // Index buffers must be a multiple of four bytes.
        if indices.len() % 2 == 1 {
            indices.push(0);
        }
        let vertex_count = base_vertex as usize;
        let index_bytes: &[u8] = if indices.is_empty() {
            &[0; 4]
        } else {
            bytemuck::cast_slice(&indices)
        };
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model indices"),
            contents: index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model vertices"),
            size: (vertex_count.max(1) * std::mem::size_of::<ModelVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let light = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model light"),
            contents: bytemuck::bytes_of(&model::LightUniform::from(ModelLight::default())),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model light"),
            layout: &self.light_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light.as_entire_binding(),
            }],
        });
        GpuModel {
            vertices,
            indices,
            draws,
            light,
            light_group,
            vertex_count,
            visible: true,
            shadow: false,
            one_sided: false,
            views,
        }
    }

    /// Lets the GPU side free what has been dropped (old textures, buffers)
    /// even when no frame is being drawn, as in headless runs.
    pub fn tidy(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    /// A picture on the GPU, ready to paint on any model's submesh: made
    /// once and painted as often as wanted, for animations that cycle a
    /// few textures over and over.
    pub fn model_texture(&self, image: &Image) -> ModelTexture {
        ModelTexture(self.texture(&color_key(image), true))
    }

    /// Paints a picture made by `model_texture` on one of a model's
    /// submeshes.
    pub fn set_model_texture(&self, model: &mut GpuModel, submesh: usize, texture: &ModelTexture) {
        if let Some(draw) = model.draws.get_mut(submesh) {
            // Made before a change of filtering, it is remade with it.
            draw.0 = self.world_group(&texture.0);
            model.views[submesh] = texture.0.clone();
        }
    }

    /// Paints a different picture on one of a model's submeshes, such as a
    /// face that has just changed expression.
    pub fn replace_model_texture(&self, model: &mut GpuModel, submesh: usize, image: &Image) {
        let Some(draw) = model.draws.get_mut(submesh) else {
            return;
        };
        let view = self.texture(&color_key(image), true);
        draw.0 = self.world_group(&view);
        model.views[submesh] = view;
    }

    /// A room or model texture's binding, with the filtering now chosen.
    fn world_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.repeat_sampler),
                },
            ],
        })
    }

    /// Chooses how room and model textures are filtered from now on.
    /// What is already on the GPU takes it with `rebind_scene` and
    /// `rebind_model`.
    pub fn set_filtering(&mut self, filtering: Filtering) {
        self.repeat_sampler = world_sampler(&self.device, filtering, self.max_anisotropy);
    }

    /// Remakes a room's texture bindings with the filtering now chosen.
    pub fn rebind_scene(&self, scene: &mut GpuScene) {
        for (view, group) in &mut scene.textures {
            *group = self.world_group(view);
        }
    }

    /// Remakes a model's texture bindings with the filtering now chosen.
    pub fn rebind_model(&self, model: &mut GpuModel) {
        for (draw, view) in model.draws.iter_mut().zip(&model.views) {
            draw.0 = self.world_group(view);
        }
    }

    /// Replaces a model's posed, placed vertices.
    pub fn update_model(&self, model: &GpuModel, vertices: &[ModelVertex]) {
        debug_assert_eq!(vertices.len(), model.vertex_count);
        if !vertices.is_empty() {
            self.queue
                .write_buffer(&model.vertices, 0, bytemuck::cast_slice(vertices));
        }
    }

    pub fn set_model_light(&self, model: &GpuModel, light: ModelLight) {
        self.queue.write_buffer(
            &model.light,
            0,
            bytemuck::bytes_of(&model::LightUniform::from(light)),
        );
    }

    /// A texture of one level, kept so it can be rewritten.
    fn texture_object(&self, image: &Image) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.write_layer(&texture, image, 0, 0);
        texture
    }

    fn texture(&self, image: &Image, mipmaps: bool) -> wgpu::TextureView {
        let mut levels = vec![image.clone()];
        if mipmaps {
            while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
                let next = mesh::half(levels.last().unwrap());
                levels.push(next);
            }
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, img) in levels.iter().enumerate() {
            self.write_layer(&texture, img, level as u32, 0);
        }
        texture.create_view(&Default::default())
    }

    fn cube_texture(&self, faces: &[Image; 6]) -> wgpu::TextureView {
        let size = faces[0].width;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, face) in faces.iter().enumerate() {
            self.write_layer(&texture, face, 0, layer as u32);
        }
        texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        })
    }

    fn write_layer(&self, texture: &wgpu::Texture, image: &Image, mip: u32, layer: u32) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: mip,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The buffers a frame of this size is drawn into.
    pub fn frame_buffers(&self, width: u32, height: u32) -> FrameBuffers {
        let buffer = |label, format| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: width.max(1),
                        height: height.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        FrameBuffers {
            depth: buffer("depth", DEPTH_FORMAT),
            multisampled: (self.samples > 1).then(|| buffer("multisampled", self.color_format)),
        }
    }

    /// The colour attachment for drawing into `target`: straight in, or
    /// into the samples, resolved into it at the end.
    fn color_attachment<'a>(
        target: &'a wgpu::TextureView,
        buffers: &'a FrameBuffers,
        clear: wgpu::Color,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        let (view, resolve_target, store) = match &buffers.multisampled {
            Some(samples) => (samples, Some(target), wgpu::StoreOp::Discard),
            None => (target, None, wgpu::StoreOp::Store),
        };
        wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store,
            },
        }
    }

    /// Draws a room and its models into `target` and submits the work.
    pub fn draw(
        &self,
        scene: &GpuScene,
        models: &[&GpuModel],
        overlay: &Overlay,
        camera: &Camera,
        mode: ShadeMode,
        target: Target,
    ) {
        let Target {
            color: target,
            depth,
            size,
        } = target;
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let rotation_only =
            camera.projection(aspect) * Mat4::look_to_lh(Vec3::ZERO, camera.forward(), Vec3::Y);
        // The sky texture turns with the azimuth, so a world direction is
        // turned back the other way before the lookup.
        let azimuth = scene.sky.as_ref().map_or(0.0, |(_, a)| *a);
        let sky_inverse = Mat4::from_rotation_y(-azimuth.to_radians()) * rotation_only.inverse();
        let globals = Globals {
            view_proj: camera.view_proj(aspect).to_cols_array_2d(),
            sky_inverse: sky_inverse.to_cols_array_2d(),
            atlas_size: scene.atlas_size,
            mode: match mode {
                ShadeMode::Lit => 0,
                ShadeMode::TexturesOnly => 1,
                ShadeMode::LightmapsOnly => 2,
            },
            _pad: 0,
        };
        self.queue
            .write_buffer(&scene.globals, 0, bytemuck::bytes_of(&globals));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(Self::color_attachment(
                    target,
                    depth,
                    wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    },
                ))],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some((sky, _)) = &scene.sky {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(0, &scene.globals_group, &[]);
                pass.set_bind_group(1, sky, &[]);
                pass.draw(0..3, 0..1);
            }

            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &scene.globals_group, &[]);
            pass.set_vertex_buffer(0, scene.vertices.slice(..));
            for batch in scene.batches.iter().filter(|b| !b.decal) {
                pass.set_bind_group(1, &scene.textures[batch.texture].1, &[]);
                pass.draw(batch.first..batch.first + batch.count, 0..1);
            }
            pass.set_pipeline(&self.decal_pipeline);
            for batch in scene.batches.iter().filter(|b| b.decal) {
                pass.set_bind_group(1, &scene.textures[batch.texture].1, &[]);
                pass.draw(batch.first..batch.first + batch.count, 0..1);
            }

            for shadows in [false, true] {
                for model in models
                    .iter()
                    .filter(|m| m.visible && m.vertex_count > 0 && m.shadow == shadows)
                {
                    pass.set_pipeline(if shadows {
                        &self.shadow_pipeline
                    } else if model.one_sided {
                        &self.one_sided_pipeline
                    } else {
                        &self.model_pipeline
                    });
                    pass.set_bind_group(2, &model.light_group, &[]);
                    pass.set_vertex_buffer(0, model.vertices.slice(..));
                    pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint16);
                    for (group, first, count, base) in &model.draws {
                        pass.set_bind_group(1, group, &[]);
                        pass.draw_indexed(*first..*first + *count, *base, 0..1);
                    }
                }
            }

            self.overlay_quads(&mut pass, overlay, size);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Draws only the overlay, over black: for screens with no room behind
    /// them, such as the card that opens a new game.
    pub fn draw_overlay(&self, overlay: &Overlay, target: Target) {
        let Target {
            color: target,
            depth,
            size,
        } = target;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("overlay frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay"),
                color_attachments: &[Some(Self::color_attachment(
                    target,
                    depth,
                    wgpu::Color::BLACK,
                ))],
                // The overlay pipeline is built for the scene's pass, which
                // has depth; it does not test against it.
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.overlay_quads(&mut pass, overlay, size);
        }
        self.queue.submit([encoder.finish()]);
    }

    fn overlay_quads(&self, pass: &mut wgpu::RenderPass<'_>, overlay: &Overlay, size: (u32, u32)) {
        if overlay.quads.is_empty() {
            return;
        }
        let sizes: Vec<(u32, u32)> = self.overlay_textures.iter().map(|(_, s)| *s).collect();
        let vertices = overlay::vertices(overlay, &sizes, size);
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("overlay"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        pass.set_pipeline(&self.overlay_pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        // Runs of quads sharing a texture are one draw.
        let mut start = 0;
        while start < overlay.quads.len() {
            let texture = overlay.quads[start].texture;
            let mut end = start + 1;
            while end < overlay.quads.len() && overlay.quads[end].texture == texture {
                end += 1;
            }
            if let Some((group, _)) = self.overlay_textures.get(texture.0) {
                pass.set_bind_group(0, group, &[]);
                pass.draw((start * 6) as u32..(end * 6) as u32, 0..1);
            }
            start = end;
        }
    }

    /// Renders one frame offscreen and reads it back.
    pub fn render_image(
        &self,
        scene: &GpuScene,
        models: &[&GpuModel],
        overlay: &Overlay,
        camera: &Camera,
        mode: ShadeMode,
        size: (u32, u32),
    ) -> Result<Image, String> {
        self.offscreen(size, |target| {
            self.draw(scene, models, overlay, camera, mode, target)
        })
    }

    /// Renders only an overlay offscreen, over black, and reads it back.
    pub fn render_overlay_image(
        &self,
        overlay: &Overlay,
        size: (u32, u32),
    ) -> Result<Image, String> {
        self.offscreen(size, |target| self.draw_overlay(overlay, target))
    }

    /// Makes an offscreen target, lets `draw` fill it, and reads it back.
    fn offscreen(
        &self,
        (width, height): (u32, u32),
        draw: impl FnOnce(Target),
    ) -> Result<Image, String> {
        use wgpu::TextureFormat as F;
        if !matches!(
            self.color_format,
            F::Rgba8Unorm | F::Rgba8UnormSrgb | F::Bgra8Unorm | F::Bgra8UnormSrgb
        ) {
            return Err(format!("cannot read back {:?} pictures", self.color_format));
        }
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth = self.frame_buffers(width, height);
        draw(Target {
            color: &view,
            depth: &depth,
            size: (width, height),
        });

        // Rows in a copy must be a multiple of 256 bytes.
        let stride = (width * 4).div_ceil(256) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        readback.map_async(wgpu::MapMode::Read, .., |_| {});
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("GPU did not finish: {e}"))?;
        let mapped = readback
            .get_mapped_range(..)
            .map_err(|e| format!("could not read the frame back: {e}"))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in mapped.chunks(stride as usize).take(height as usize) {
            rgba.extend_from_slice(&row[..(width * 4) as usize]);
        }
        // A window's format is often blue first.
        if matches!(
            self.color_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}

/// GK3's transparency: magenta. G-Engine discards near-magenta texels in every
/// texture (within 0.1 in RGB), not just those marked by a magenta corner, and
/// leaves and chains depend on it. Keyed texels also take the colour of their
/// opaque neighbours, so filtering at a cut-out's edge blends towards the
/// leaf rather than towards purple.
pub fn color_key(image: &Image) -> Image {
    let is_key = |p: &[u8]| p[0] >= 230 && p[1] <= 25 && p[2] >= 230;
    let mut out = image.clone();
    let (w, h) = (image.width as i64, image.height as i64);
    let mut any = false;
    for (i, p) in image.rgba.chunks_exact(4).enumerate() {
        if p[3] != 0 && !is_key(p) {
            continue;
        }
        any = true;
        let (x, y) = (i as i64 % w, i as i64 / w);
        let mut sum = [0u32; 3];
        let mut n = 0;
        for (dx, dy) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
        ] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let j = ((ny * w + nx) * 4) as usize;
            let q = &image.rgba[j..j + 4];
            if q[3] != 0 && !is_key(q) {
                for c in 0..3 {
                    sum[c] += q[c] as u32;
                }
                n += 1;
            }
        }
        let o = &mut out.rgba[i * 4..i * 4 + 4];
        if n > 0 {
            for c in 0..3 {
                o[c] = (sum[c] / n) as u8;
            }
        } else {
            o[..3].copy_from_slice(&[0, 0, 0]);
        }
        o[3] = 0;
    }
    if any { out } else { image.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window's renderer often draws blue-first: a picture read back from
    /// it is still the right colours (and drawing it must not fail).
    #[test]
    fn pictures_read_back_from_a_window_renderer() {
        let Ok((device, queue, _)) = headless() else {
            eprintln!("no GPU; skipped");
            return;
        };
        for format in [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm] {
            // Multisampled too: the samples resolve into the picture.
            for samples in [1, MSAA_SAMPLES] {
                let renderer =
                    Renderer::with_samples(device.clone(), queue.clone(), format, samples);
                let mut overlay = Overlay::default();
                overlay.rect([0.0, 0.0, 4.0, 4.0], [255, 0, 0, 255]);
                let image = renderer.render_overlay_image(&overlay, (4, 4)).unwrap();
                assert_eq!(&image.rgba[..4], &[255, 0, 0, 255], "{format:?} x{samples}");
            }
        }
    }

    #[test]
    fn magenta_becomes_transparent_with_neighbour_colour() {
        let image = Image {
            width: 3,
            height: 1,
            rgba: vec![0, 200, 0, 255, 255, 0, 255, 255, 0, 100, 0, 255],
        };
        let keyed = color_key(&image);
        assert_eq!(&keyed.rgba[4..8], &[0, 150, 0, 0]);
        assert_eq!(&keyed.rgba[0..4], &[0, 200, 0, 255]);
    }
}
