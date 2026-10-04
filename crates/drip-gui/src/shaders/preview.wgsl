// Draws a linear Rec.2020 image into the canvas in egui's encoding: linear
// sRGB coordinates (negative outside the sRGB gamut) through the sRGB curve,
// extended to all reals by symmetry.

@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    // One triangle covering the viewport, which egui sets to the image rect.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

// Rec.2020 to BT.709/sRGB primaries, both D65 [ITU-R BT.2407-0, eq. (1)], column-major.
const TO_SRGB = mat3x3<f32>(
    vec3<f32>(1.6605, -0.1246, -0.0182),
    vec3<f32>(-0.5876, 1.1329, -0.1006),
    vec3<f32>(-0.0728, -0.0083, 1.1187),
);

fn encode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let gamma = select(1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - 0.055, a * 12.92, a <= vec3<f32>(0.0031308));
    return sign(v) * gamma;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(encode(TO_SRGB * textureSample(image, image_sampler, in.uv).rgb), 1.0);
}
