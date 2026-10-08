// FP32 Rec.2020 samples remain in their compute storage buffer. Filtering is
// explicit so scalar-packed RGB needs no intermediate texture or host readback.
@group(0) @binding(0) var<storage, read> image: array<f32>;
// width, height, interpolation flag, unused
@group(0) @binding(1) var<uniform> info: vec4<u32>;
// Whole image in normalized device coordinates: top-left and bottom-right.
@group(0) @binding(2) var<uniform> rect: vec4<f32>;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let uv = corners[i];
    return Out(vec4<f32>(mix(rect.xy, rect.zw, uv), 0.0, 1.0), uv);
}

fn pixel(position: vec2<i32>) -> vec3<f32> {
    let p = vec2<u32>(clamp(position, vec2(0), vec2<i32>(info.xy) - vec2(1)));
    let offset = (p.y * info.x + p.x) * 3u;
    return vec3(image[offset], image[offset + 1u], image[offset + 2u]);
}

fn sample_image(uv: vec2<f32>) -> vec3<f32> {
    let position = uv * vec2<f32>(info.xy);
    if info.z == 0u { return pixel(vec2<i32>(floor(position))); }
    // Texel centers lie at half-integers. Clamp each tap, matching the previous
    // clamp-to-edge texture sampler even for a one-pixel image.
    let centered = position - vec2(0.5);
    let base = vec2<i32>(floor(centered));
    let fraction = fract(centered);
    let top = mix(pixel(base), pixel(base + vec2(1, 0)), fraction.x);
    let bottom = mix(pixel(base + vec2(0, 1)), pixel(base + vec2(1, 1)), fraction.x);
    return mix(top, bottom, fraction.y);
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    // SDR display clipping is a presentation decision. It does not modify the
    // processing buffer, whose negative/HDR values remain available for export.
    let rgb = clamp(sample_image(in.uv), vec3(0.0), vec3(1.0));
    return vec4<f32>(encode(TO_SRGB * rgb), 1.0);
}
