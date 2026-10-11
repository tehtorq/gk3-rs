// Models: characters and props. Positions arrive already posed and placed in
// the world each frame; lighting is an ambient term plus one light.

struct Globals {
    view_proj: mat4x4<f32>,
    sky_inverse: mat4x4<f32>,
    atlas_size: vec2<f32>,
    mode: u32,
    _pad: u32,
};

struct ModelLight {
    // Direction towards the light, in world space.
    direction: vec4<f32>,
    color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var diffuse: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;
@group(2) @binding(0) var<uniform> light: ModelLight;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = g.view_proj * vec4<f32>(v.position, 1.0);
    out.normal = v.normal;
    out.uv = v.uv;
    out.tint = v.tint;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(diffuse, diffuse_sampler, in.uv) * vec4<f32>(in.tint.rgb, 1.0);
    if texel.a < 0.5 {
        discard;
    }
    let n = normalize(in.normal);
    // Normals in the data are not consistently outward, so light both sides.
    let lambert = abs(dot(n, light.direction.xyz));
    let lit = light.ambient.rgb + light.color.rgb * lambert;
    if g.mode == 1u {
        return vec4<f32>(texel.rgb, 1.0);
    }
    return vec4<f32>(texel.rgb * lit, 1.0);
}
