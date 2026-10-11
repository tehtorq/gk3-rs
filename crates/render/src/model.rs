//! Models on the GPU: characters and props.
//!
//! GK3 animates by moving vertices, so a model's vertex buffer is rewritten
//! whenever its pose changes; the index buffer and textures stay put.

use gk3_formats::image::Image;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    /// World space.
    pub position: [f32; 3],
    /// World space.
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tint: [u8; 4],
}

/// One textured part of a model, as uploaded.
pub struct SubmeshDesc<'a> {
    pub texture: Image,
    pub tint: [u8; 3],
    pub triangles: &'a [[u16; 3]],
    pub vertex_count: usize,
}

/// How a model is lit this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLight {
    /// Towards the light, world space.
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub ambient: [f32; 3],
}

impl Default for ModelLight {
    fn default() -> Self {
        Self {
            direction: [0.3, 0.9, 0.3],
            color: [0.45, 0.45, 0.45],
            ambient: [0.6, 0.6, 0.6],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightUniform {
    pub direction: [f32; 4],
    pub color: [f32; 4],
    pub ambient: [f32; 4],
}

impl From<ModelLight> for LightUniform {
    fn from(l: ModelLight) -> Self {
        let [x, y, z] = l.direction;
        let len = (x * x + y * y + z * z).sqrt().max(1e-6);
        Self {
            direction: [x / len, y / len, z / len, 0.0],
            color: [l.color[0], l.color[1], l.color[2], 1.0],
            ambient: [l.ambient[0], l.ambient[1], l.ambient[2], 1.0],
        }
    }
}

pub struct GpuModel {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    /// Per submesh: texture bind group, first index, index count, base vertex.
    pub(crate) draws: Vec<(wgpu::BindGroup, u32, u32, i32)>,
    pub(crate) light: wgpu::Buffer,
    pub(crate) light_group: wgpu::BindGroup,
    pub vertex_count: usize,
    pub visible: bool,
}
