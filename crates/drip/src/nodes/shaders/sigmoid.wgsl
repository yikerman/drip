// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2020-2026 darktable developers.
// [1] darktable developers, "sigmoid.c" and "custom_primaries.c," commit
// 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3. [Online]. Available:
// https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src
// See THIRD_PARTY.md for credits and upstream license. Coefficients use host f64.
@group(0) @binding(0) var<storage, read> source: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<f32>;

fn apply(offset:u32,v:vec3<f32>)->vec3<f32> {
    return vec3(
        p[offset]*v.x+p[offset+1u]*v.y+p[offset+2u]*v.z,
        p[offset+3u]*v.x+p[offset+4u]*v.y+p[offset+5u]*v.z,
        p[offset+6u]*v.x+p[offset+7u]*v.y+p[offset+8u]*v.z);
}
fn curve(v:f32,log_scale:f32)->f32 {
    if v<=0.0 {return 0.0;}
    let z=p[2]-p[0]*(log(v)+log_scale);
    let softplus=max(z,0.0)+log(1.0+exp(-abs(z)));
    return exp(-p[1]*softplus);
}
fn positive(v:vec3<f32>)->vec3<f32> {
    let average=max(v.x/3.0+v.y/3.0+v.z/3.0,0.0);
    let minimum=min(v.x,min(v.y,v.z));
    var saturation=1.0;
    if minimum<0.0 {
        // Ratio form avoids overflow of average-minimum for finite HDR values.
        saturation=0.0;
        if average>0.0 {saturation=1.0/(1.0-minimum/average);}
    }
    return max(vec3(0.0),(1.0-saturation)*average+saturation*v);
}
fn preserve_hue(v:vec3<f32>,mapped:vec3<f32>)->vec3<f32> {
    var order=vec3(0u,1u,2u);
    if v[order.x]>v[order.y] {let t=order.x;order.x=order.y;order.y=t;}
    if v[order.y]>v[order.z] {let t=order.y;order.y=order.z;order.z=t;}
    if v[order.x]>v[order.y] {let t=order.x;order.x=order.y;order.y=t;}
    let lo=order.x; let mid=order.y; let hi=order.z;
    let chroma=v[hi]-v[lo];
    var fraction=0.0;
    if chroma!=0.0 {fraction=(v[mid]-v[lo])/chroma;}
    let hue=p[3];
    let corrected=mapped[lo]+(mapped[hi]-mapped[lo])*fraction;
    let naive=(1.0-hue)*mapped[mid]+hue*corrected;
    var blend=0.0;
    let largest=max(v[lo],v[mid]);
    if largest>0.0 {blend=2.0*(v[lo]/largest)/(v[lo]/largest+v[mid]/largest);}
    let energy=blend*(mapped.x+mapped.y+mapped.z)+(1.0-blend)*(mapped[lo]+naive+mapped[hi]);
    var result=mapped;
    if naive<=mapped[mid] {
        result[mid]=((1.0-hue)*mapped[mid]+hue*(fraction*mapped[hi]+(1.0-fraction)*(energy-mapped[hi])))/(1.0+hue*(1.0-fraction));
        result[lo]=energy-mapped[hi]-result[mid];
    } else {
        result[mid]=((1.0-hue)*mapped[mid]+hue*(mapped[lo]*(1.0-fraction)+fraction*(energy-mapped[lo])))/(1.0+hue*fraction);
        result[hi]=energy-mapped[lo]-result[mid];
    }
    return result;
}
@compute @workgroup_size(256)
fn sigmoid(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let i=(id.x+id.y*groups.x*256u)*3u;
    if i+2u>=arrayLength(&source) {return;}
    let input=vec3(source[i],source[i+1u],source[i+2u]);
    let peak=max(abs(input.x),max(abs(input.y),abs(input.z)));
    // Positive projection and matrix conversion are homogeneous. Normalize only
    // extreme HDR inputs so their intermediates cannot overflow; carry the
    // common factor into the curve in log space. Hue ratios ignore that factor.
    // Keep the divisor below 1/min-normal: a reciprocal of f32::MAX can
    // flush to zero on hardware even when the quotient is representable.
    let scale=select(1.0,1e20,peak>1e20);
    let v=apply(4u,positive(input/scale));
    let log_scale=log(scale);
    let mapped=vec3(curve(v.x,log_scale),curve(v.y,log_scale),curve(v.z,log_scale));
    let result=apply(13u,preserve_hue(v,mapped));
    output[i]=result.x;output[i+1u]=result.y;output[i+2u]=result.z;
}
