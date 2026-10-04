// Copies the gamma-encoded canvas egui and the previews were drawn into onto
// the swapchain. In scRGB mode, decodes the extended sRGB encoding (sign
// preserving, so values outside [0, 1] survive) to linear and scales SDR
// white to the compositor's reference white; otherwise clamps to sRGB.

struct Params { mode: u32, white: f32 }

@group(0) @binding(0) var canvas: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn decode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let linear = select(pow((a + 0.055) / 1.055, vec3<f32>(2.4)), a / 12.92, a <= vec3<f32>(0.04045));
    return sign(v) * linear;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(canvas, vec2<i32>(pos.xy), 0).rgb;
    if params.mode == 0u {
        return vec4<f32>(decode(c) * params.white, 1.0);
    }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
