// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2022-2026 darktable developers.
// Inpaint opposed: garagecoder, Iain (G'MIC), Hanno Schwalm (darktable).
// [1] darktable developers, "opposed.c" and "segbased.c," commit
// 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3. [Online]. Available:
// https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct
// See THIRD_PARTY.md for credits and upstream license.
// WGSL lacks f64: deterministic compensated f32 sums replace f64 reductions.
@group(0) @binding(0) var<storage, read> source: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<storage, read_write> mask: array<f32>;
@group(0) @binding(3) var<storage, read_write> near: array<f32>;
@group(0) @binding(4) var<storage, read_write> rows: array<f32>;
@group(0) @binding(5) var<storage, read_write> chroma: array<f32>;
@group(0) @binding(6) var<storage, read> p: array<f32>;
fn width()->i32 {return i32(p[0]);}
fn height()->i32 {return i32(p[1]);}
fn sensor(pos:vec2<i32>)->u32 {return u32(p[2u+u32(pos.y%2*2+pos.x%2)]);}
fn channel(ch:u32)->u32 {return select(ch,1u,ch==3u);}
fn value(pos:vec2<i32>)->f32 {return source[u32(pos.y*width()+pos.x)];}
fn reference(pos:vec2<i32>)->f32 {
    var sums=vec3(0.0);var counts=vec3(0.0);
    for(var r=max(0,pos.y-1);r<=min(height()-1,pos.y+1);r++) {
        for(var c=max(0,pos.x-1);c<=min(width()-1,pos.x+1);c++) {
            let q=vec2(c,r);let ch=channel(sensor(q));
            sums[ch]+=max(0.0,value(q));counts[ch]+=1.0;
        }
    }
    let means=pow(sums/max(counts,vec3(1.0)),vec3(1.0/3.0));
    let ch=channel(sensor(pos));let opposite=0.5*(means[(ch+1u)%3u]+means[(ch+2u)%3u]);
    return opposite*opposite*opposite;
}
@compute @workgroup_size(256)
fn opposed(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let i=id.x+id.y*groups.x*256u;let stage=u32(p[10]);let mw=(width()+2)/3;let mh=(height()+2)/3;
    if stage==0u {
        if i>=u32(mw*mh) {return;}
        let origin=vec2(i32(i)%mw*3,i32(i)/mw*3);var hits=vec3(0.0);
        for(var r=origin.y;r<min(origin.y+3,height());r++) {
            for(var c=origin.x;c<min(origin.x+3,width());c++) {
                let q=vec2(c,r);let ch=sensor(q);
                if value(q)>=p[6u+ch] {hits[channel(ch)]=1.0;}
            }
        }
        for(var ch=0u;ch<3u;ch++) {mask[3u*i+ch]=hits[ch];}return;
    }
    if stage==1u {
        if i>=u32(mw*mh) {return;}
        let pos=vec2(i32(i)%mw,i32(i)/mw);var hits=vec3(0.0);
        for(var r=max(0,pos.y-3);r<=min(mh-1,pos.y+3);r++) {
            for(var c=max(0,pos.x-3);c<=min(mw-1,pos.x+3);c++) {
                if abs(r-pos.y)==3 && abs(c-pos.x)==3 {continue;}
                for(var ch=0u;ch<3u;ch++) {hits[ch]=max(hits[ch],mask[u32(r*mw+c)*3u+ch]);}
            }
        }
        for(var ch=0u;ch<3u;ch++) {near[3u*i+ch]=hits[ch];}return;
    }
    if stage==2u {
        if i>=u32(height()) {return;}
        var sums=vec3(0.0);var errors=vec3(0.0);var counts=vec3(0u);
        for(var c=0;c<width();c++) {
            let pos=vec2(c,i32(i));let ch=sensor(pos);let rgb=channel(ch);let v=value(pos);
            if v>0.2*p[6u+ch] && v<p[6u+ch] && near[u32(i32(i)/3*mw+c/3)*3u+rgb]!=0.0 {
                let delta=(v-reference(pos))-errors[rgb];let next=sums[rgb]+delta;
                errors[rgb]=(next-sums[rgb])-delta;sums[rgb]=next;counts[rgb]+=1u;
            }
        }
        for(var ch=0u;ch<3u;ch++) {rows[i*6u+ch]=sums[ch];rows[i*6u+3u+ch]=f32(counts[ch]);}return;
    }
    if stage==3u {
        if i!=0u {return;}
        var sums=vec3(0.0);var errors=vec3(0.0);var counts=vec3(0u);
        for(var r=0u;r<u32(height());r++) {
            for(var ch=0u;ch<3u;ch++) {
                let delta=rows[r*6u+ch]-errors[ch];let next=sums[ch]+delta;
                errors[ch]=(next-sums[ch])-delta;sums[ch]=next;counts[ch]+=u32(rows[r*6u+3u+ch]);
            }
        }
        for(var ch=0u;ch<3u;ch++) {
            var offset=0.0;if counts[ch]>100u {offset=sums[ch]/f32(counts[ch]);}chroma[ch]=offset;
        }return;
    }
    if i>=u32(width()*height()) {return;}
    let pos=vec2(i32(i)%width(),i32(i)/width());let ch=sensor(pos);let v=source[i];
    output[i]=v;
    if v>=p[6u+ch] {output[i]=max(v,reference(pos)+chroma[channel(ch)]);}
}
