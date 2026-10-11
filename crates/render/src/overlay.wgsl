// The interface: textured, tinted quads already in clip space.

@group(0) @binding(0) var sheet: texture_2d<f32>;
@group(0) @binding(1) var sheet_sampler: sampler;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(v.position, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(sheet, sheet_sampler, in.uv) * in.color;
}
