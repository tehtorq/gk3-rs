// Skybox: one triangle covering the screen, looking up a cube map by the
// direction through each pixel.

struct Globals {
    view_proj: mat4x4<f32>,
    sky_inverse: mat4x4<f32>,
    atlas_size: vec2<f32>,
    mode: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var sky: texture_cube<f32>;
@group(1) @binding(1) var sky_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: VertexOut;
    out.clip = vec4<f32>(xy, 1.0, 1.0);
    out.ndc = xy;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let far = g.sky_inverse * vec4<f32>(in.ndc, 1.0, 1.0);
    let near = g.sky_inverse * vec4<f32>(in.ndc, 0.0, 1.0);
    let dir = far.xyz / far.w - near.xyz / near.w;
    return vec4<f32>(textureSample(sky, sky_sampler, dir).rgb, 1.0);
}
