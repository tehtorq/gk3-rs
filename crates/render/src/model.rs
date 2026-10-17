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

/// How a model is lit this frame, as the original lights it (0x559550): a
/// vertex is its tint times the light colour plus (2 cos - 1) times the
/// range, cos the light's on the surface's front (none behind), clamped to
/// 0 to 1; the texture is then modulated by it twice over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLight {
    /// Towards the light, world space.
    pub direction: [f32; 3],
    /// The light colour.
    pub colour: [f32; 3],
    /// How far the light swings the shading either way.
    pub range: f32,
}

impl Default for ModelLight {
    /// The original's default light colour (half) and range (0.3), as its
    /// hardware renderer takes them with its doubled modulation: the range
    /// halved.
    fn default() -> Self {
        Self {
            direction: [0.3, 0.9, 0.3],
            colour: [0.5; 3],
            range: 0.15,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightUniform {
    pub direction: [f32; 4],
    /// The light colour, and the range in `w`.
    pub colour: [f32; 4],
}

impl From<ModelLight> for LightUniform {
    fn from(l: ModelLight) -> Self {
        let [x, y, z] = l.direction;
        let len = (x * x + y * y + z * z).sqrt().max(1e-6);
        Self {
            direction: [x / len, y / len, z / len, 0.0],
            colour: [l.colour[0], l.colour[1], l.colour[2], l.range],
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
    /// A blob shadow, multiplied onto the floor after everything else.
    pub shadow: bool,
    /// Back faces are not drawn (characters); otherwise both sides are.
    pub one_sided: bool,
    /// Each submesh's texture, for remaking its binding.
    pub(crate) views: Vec<wgpu::TextureView>,
}
