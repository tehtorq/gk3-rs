// Models: characters and props. Positions arrive already posed and placed in
// the world each frame; lighting is the original's (0x559550): a vertex is
// its tint times the light colour plus (2 cos - 1) times the range, cos the
// light on the surface's front, clamped to 0 to 1; its hardware renderer then
// modulates the texture by that twice over (MODULATE2X).

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
    // The light colour; w is the range.
    colour: vec4<f32>,
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
    let texel = textureSample(diffuse, diffuse_sampler, in.uv);
    if texel.a < 0.5 {
        discard;
    }
    if g.mode == 1u {
        return vec4<f32>(texel.rgb * in.tint.rgb, 1.0);
    }
    let n = normalize(in.normal);
    let lambert = max(dot(n, light.direction.xyz), 0.0);
    let lit = clamp(
        in.tint.rgb * (light.colour.rgb + (2.0 * lambert - 1.0) * light.colour.w),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    return vec4<f32>(min(2.0 * texel.rgb * lit, vec3<f32>(1.0)), 1.0);
}

// A blob shadow, as the original draws it on a card that can modulate twice
// over (0x599f20, 0x59ae00): its picture doubled, then multiplied onto what
// lies under it.
@fragment
fn fs_shadow(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(diffuse, diffuse_sampler, in.uv);
    return vec4<f32>(min(texel.rgb * 2.0, vec3<f32>(1.0)), 1.0);
}
