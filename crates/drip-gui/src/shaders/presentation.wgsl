@group(0) @binding(0) var canvas: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// Bound the final composite, since blending encoded colors can leave the
// Rec.2020 volume even when each input is inside it. Never clamp BT.709 here:
// its negative and above-one components carry the wider gamut.
fn bounded_linear(pos: vec4<f32>) -> vec3<f32> {
    let rgb = decode(textureLoad(canvas, vec2<i32>(pos.xy), 0).rgb);
    return TO_SRGB * clamp(TO_REC2020 * rgb, vec3(0.0), vec3(1.0));
}

@fragment
fn linear(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(bounded_linear(pos), 1.0);
}

// Relative-colorimetric sRGB fallback: D65 is shared, and destination clipping
// follows the matrix conversion. Checked against LittleCMS RGB16 output.
@fragment
fn srgb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(encode(clamp(bounded_linear(pos), vec3(0.0), vec3(1.0))), 1.0);
}

// A hardware-sRGB attachment performs the encoding on write.
@fragment
fn srgb_linear(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(clamp(bounded_linear(pos), vec3(0.0), vec3(1.0)), 1.0);
}
