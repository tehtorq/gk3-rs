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
    pub depth: &'a wgpu::TextureView,
    pub size: (u32, u32),
}

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
    sky_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    light_layout: wgpu::BindGroupLayout,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_textures: Vec<(wgpu::BindGroup, (u32, u32))>,
    globals_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sky_layout: wgpu::BindGroupLayout,
    repeat_sampler: wgpu::Sampler,
    clamp_sampler: wgpu::Sampler,
}

/// A room on the GPU.
pub struct GpuScene {
    vertices: wgpu::Buffer,
    batches: Vec<mesh::Batch>,
    textures: Vec<wgpu::BindGroup>,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    sky: Option<(wgpu::BindGroup, f32)>,
    atlas_size: [f32; 2],
    pub bounds: ([f32; 3], [f32; 3]),
}

impl Renderer {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
    ) -> Self {
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

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
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
            // GK3's winding is not consistent, so nothing is culled.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });

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
            multisample: Default::default(),
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
        let model_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model"),
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
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &model_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target,
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
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];
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
            multisample: Default::default(),
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

        let repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
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
            sky_pipeline,
            model_pipeline,
            light_layout,
            overlay_pipeline,
            overlay_textures: Vec::new(),
            globals_layout,
            texture_layout,
            sky_layout,
            repeat_sampler,
            clamp_sampler,
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
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.repeat_sampler),
                        },
                    ],
                })
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
        let view = self.texture(image, false);
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
        self.overlay_textures
            .push((group, (image.width, image.height)));
        OverlayTexture(self.overlay_textures.len() - 1)
    }

    /// Uploads a model's textures and triangles. Its vertices are supplied
    /// with [`Renderer::update_model`], in world space, whenever it moves.
    pub fn upload_model(&self, submeshes: &[SubmeshDesc]) -> GpuModel {
        let device = &self.device;
        let mut indices: Vec<u16> = Vec::new();
        let mut draws = Vec::new();
        let mut base_vertex = 0i32;
        for sub in submeshes {
            let view = self.texture(&color_key(&sub.texture), true);
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.repeat_sampler),
                    },
                ],
            });
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

    /// A depth buffer for a target of this size.
    pub fn depth_view(&self, width: u32, height: u32) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
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
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
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
            for batch in &scene.batches {
                pass.set_bind_group(1, &scene.textures[batch.texture], &[]);
                pass.draw(batch.first..batch.first + batch.count, 0..1);
            }

            pass.set_pipeline(&self.model_pipeline);
            for model in models.iter().filter(|m| m.visible && m.vertex_count > 0) {
                pass.set_bind_group(2, &model.light_group, &[]);
                pass.set_vertex_buffer(0, model.vertices.slice(..));
                pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint16);
                for (group, first, count, base) in &model.draws {
                    pass.set_bind_group(1, group, &[]);
                    pass.draw_indexed(*first..*first + *count, *base, 0..1);
                }
            }

            if !overlay.quads.is_empty() {
                let sizes: Vec<(u32, u32)> =
                    self.overlay_textures.iter().map(|(_, s)| *s).collect();
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
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Renders one frame offscreen and reads it back.
    pub fn render_image(
        &self,
        scene: &GpuScene,
        models: &[&GpuModel],
        overlay: &Overlay,
        camera: &Camera,
        mode: ShadeMode,
        (width, height): (u32, u32),
    ) -> Result<Image, String> {
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
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth = self.depth_view(width, height);
        self.draw(
            scene,
            models,
            overlay,
            camera,
            mode,
            Target {
                color: &view,
                depth: &depth,
                size: (width, height),
            },
        );

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
