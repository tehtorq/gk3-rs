// Room geometry: diffuse texture multiplied by a lightmap, in gamma space as
// the original did.

struct Globals {
    view_proj: mat4x4<f32>,
    // Inverse of the rotation-only view-projection, with the skybox azimuth
    // folded in, for turning a screen position into a sky direction.
    sky_inverse: mat4x4<f32>,
    atlas_size: vec2<f32>,
    // 0: textured and lit, 1: textures only, 2: lightmaps only.
    mode: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var lightmap_atlas: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;
@group(1) @binding(0) var diffuse: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lightmap_uv: vec2<f32>,
    @location(3) lightmap_rect: vec4<f32>,
    @location(4) lightmap_multiplier: f32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lightmap_uv: vec2<f32>,
    @location(2) @interpolate(flat) lightmap_rect: vec4<f32>,
    @location(3) @interpolate(flat) lightmap_multiplier: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = g.view_proj * vec4<f32>(v.position, 1.0);
    out.uv = v.uv;
    out.lightmap_uv = v.lightmap_uv;
    out.lightmap_rect = v.lightmap_rect;
    out.lightmap_multiplier = v.lightmap_multiplier;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(diffuse, diffuse_sampler, in.uv);

    // Stay half a texel inside this surface's rectangle so bilinear
    // filtering never reaches a neighbour in the atlas.
    let size = max(in.lightmap_rect.zw, vec2<f32>(1.0));
    let t = clamp(in.lightmap_uv * size, vec2<f32>(0.5), size - vec2<f32>(0.5));
    let atlas_uv = (in.lightmap_rect.xy + t) / g.atlas_size;
    let lit = textureSample(lightmap_atlas, lightmap_sampler, atlas_uv).rgb;

    // Colour-keyed texels decode with zero alpha.
    if texel.a < 0.5 {
        discard;
    }

    var light = vec3<f32>(1.0);
    if in.lightmap_multiplier > 0.0 {
        light = lit * in.lightmap_multiplier;
    }

    var color = texel.rgb * light;
    if g.mode == 1u {
        color = texel.rgb;
    } else if g.mode == 2u {
        color = light * 0.5;
    }
    return vec4<f32>(color, 1.0);
}
