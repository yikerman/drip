// Decode the extended-sRGB canvas before converting to the output primaries.
// Preserve extended BT.709 coordinates until destination clipping.

@group(0) @binding(0) var canvas: texture_2d<f32>;

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
fn linear(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(decode(textureLoad(canvas, vec2<i32>(pos.xy), 0).rgb), 1.0);
}

// BT.709 -> Rec.2020, both D65, derived from the primaries [2–4 in
// THIRD_PARTY.md]. Column-major. This also converts egui's sRGB colors.
const TO_REC2020 = mat3x3<f32>(
    vec3<f32>(0.62740390, 0.06909729, 0.01639144),
    vec3<f32>(0.32928304, 0.91954040, 0.08801331),
    vec3<f32>(0.04331307, 0.01136232, 0.89559525),
);

@fragment
fn rec2020(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let rgb = TO_REC2020 * decode(textureLoad(canvas, vec2<i32>(pos.xy), 0).rgb);
    // Bound FP16 round-trip error to the Wayland description's SDR volume.
    return vec4(clamp(rgb, vec3(0.0), vec3(1.0)), 1.0);
}

// Rec.2020 -> BT.709 was applied by the preview shader. Both spaces use D65
// and zero black, so the matrix/shaper relative-colorimetric sRGB conversion
// ends with destination clipping. Tested against LittleCMS's RGB16 output.
fn bounded(pos: vec4<f32>) -> vec3<f32> {
    return clamp(textureLoad(canvas, vec2<i32>(pos.xy), 0).rgb, vec3(0.0), vec3(1.0));
}

@fragment
fn srgb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(bounded(pos), 1.0);
}

@fragment
fn srgb_linear(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(decode(bounded(pos)), 1.0);
}
