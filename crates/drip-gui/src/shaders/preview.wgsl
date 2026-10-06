// Draws a linear Rec.2020 image into the canvas in egui's encoding: linear
// sRGB coordinates (negative outside the sRGB gamut) through the sRGB curve,
// extended to all reals by symmetry.

@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
// Where the whole image goes, in normalized device coordinates: top-left and
// bottom-right corners. egui's scissor clips it to the visible part.
@group(0) @binding(2) var<uniform> rect: vec4<f32>;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    // Two triangles covering the image rect.
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let uv = corners[i];
    return Out(vec4<f32>(mix(rect.xy, rect.zw, uv), 0.0, 1.0), uv);
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    // SDR preview only. Clip in Rec.2020 before conversion so colors outside
    // BT.709 survive in the extended canvas. Processing/export are unchanged.
    let rgb = clamp(textureSample(image, image_sampler, in.uv).rgb, vec3(0.0), vec3(1.0));
    return vec4<f32>(encode(TO_SRGB * rgb), 1.0);
}
