// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2010-2026 darktable developers.
// [1] L. Sanz Rodríguez, I. Weyrich, H. Schwalm et al., "Ratio Corrected
// Demosaicing," darktable, rcd.c, commit
// 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3. [Online]. Available:
// https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/demosaicing/rcd.c
// See THIRD_PARTY.md for credits and upstream license.
// Full-image passes retain the upstream directional/ratio estimates. The outer
// ten pixels use Drip's measured-sample-preserving bilinear boundary rule.
@group(0) @binding(0) var<storage, read> source: array<f32>;
@group(0) @binding(1) var<storage, read> previous: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
@group(0) @binding(3) var<storage, read_write> maps: array<f32>;
@group(0) @binding(4) var<storage, read> p: array<f32>;
const EPS:f32=0.00001;
fn width()->i32 {return i32(p[0]);}
fn height()->i32 {return i32(p[1]);}
fn index(pos:vec2<i32>)->u32 {return u32(pos.y*width()+pos.x);}
fn channel(pos:vec2<i32>)->u32 {return u32(p[2u+u32((pos.y%2)*2+pos.x%2)]);}
fn raw(pos:vec2<i32>)->f32 {return max(0.0,source[index(pos)]);}
fn rgb(pos:vec2<i32>,ch:u32)->f32 {return previous[index(pos)*3u+ch];}
fn inside(pos:vec2<i32>,border:i32)->bool {
    return pos.x>=border && pos.y>=border && pos.x+border<width() && pos.y+border<height();
}
fn high(pos:vec2<i32>,d:vec2<i32>)->f32 {
    let v=(raw(pos-3*d)-raw(pos-d)-raw(pos+d)+raw(pos+3*d))-3.0*(raw(pos-2*d)+raw(pos+2*d))+6.0*raw(pos);
    return v*v;
}
fn diagonal(pos:vec2<i32>,d:vec2<i32>)->f32 {
    if !inside(pos,3) || pos.x%2!=1 {return 0.0;}
    return high(pos,d);
}
fn direction(pos:vec2<i32>,offset:u32)->f32 {
    let a=maps[offset+index(pos)];
    let neighbor=0.25*(maps[offset+index(pos+vec2(-1,-1))]+maps[offset+index(pos+vec2(1,-1))]+maps[offset+index(pos+vec2(-1,1))]+maps[offset+index(pos+vec2(1,1))]);
    return select(a,neighbor,abs(0.5-a)<abs(0.5-neighbor));
}
fn weighted(a:vec2<f32>,b:vec2<f32>)->f32 {return (a.x*b.y+b.x*a.y)/(a.x+b.x);}
fn blend(t:f32,a:f32,b:f32)->f32 {let u=clamp(t,0.0,1.0);return u*a+(1.0-u)*b;}
fn green_estimate(pos:vec2<i32>,d:vec2<i32>)->vec2<f32> {
    let n=u32(width()*height());
    let grad=EPS+abs(raw(pos+d)-raw(pos-d))+abs(raw(pos)-raw(pos+2*d))+abs(raw(pos+d)-raw(pos+3*d))+abs(raw(pos+2*d)-raw(pos+4*d));
    let low=maps[n+index(pos)];
    let value=raw(pos+d)*(low+low)/(EPS+low+maps[n+index(pos+2*d)]);
    return vec2(grad,value);
}
fn color_estimate(pos:vec2<i32>,d:vec2<i32>,ch:u32)->vec2<f32> {
    let grad=EPS+abs(rgb(pos,1u)-rgb(pos+2*d,1u))+abs(rgb(pos+d,ch)-rgb(pos-d,ch))+abs(rgb(pos+d,ch)-rgb(pos+3*d,ch));
    return vec2(grad,rgb(pos+d,ch)-rgb(pos+d,1u));
}
fn bilinear(pos:vec2<i32>)->vec3<f32> {
    var sum=vec3(0.0);var count=vec3(0.0);
    for(var r=max(0,pos.y-1);r<=min(height()-1,pos.y+1);r++) {
        for(var c=max(0,pos.x-1);c<=min(width()-1,pos.x+1);c++) {
            let q=vec2(c,r);let ch=channel(q);sum[ch]+=raw(q);count[ch]+=1.0;
        }
    }
    var result=sum/max(count,vec3(1.0));result[channel(pos)]=raw(pos);return result;
}
@compute @workgroup_size(256)
fn rcd(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let i=id.x+id.y*groups.x*256u;let n=u32(width()*height());if i>=n {return;}
    let pos=vec2(i32(i)%width(),i32(i)/width());let ch=channel(pos);let stage=u32(p[6]);
    if stage==0u {
        var initial=vec3(0.0);let v=raw(pos);
        initial[channel(vec2(0,pos.y))]=v;initial[channel(vec2(1,pos.y))]=v;
        for(var c=0u;c<3u;c++) {output[3u*i+c]=initial[c];}
        var vh=0.0;var low=0.0;var pq=0.0;
        if inside(pos,4) {
            let vertical=max(EPS*EPS,high(pos-vec2(0,1),vec2(0,1))+high(pos,vec2(0,1))+high(pos+vec2(0,1),vec2(0,1)));
            let horizontal=max(EPS*EPS,high(pos-vec2(1,0),vec2(1,0))+high(pos,vec2(1,0))+high(pos+vec2(1,0),vec2(1,0)));
            vh=vertical/(vertical+horizontal);
            if ch!=1u {
                let a=vec2((pos.x-1)|1,pos.y-1);let b=vec2(pos.x|1,pos.y);let d=vec2((pos.x-1)|1,pos.y+1);
                let ps=max(EPS*EPS,diagonal(a,vec2(1,1))+diagonal(b,vec2(1,1))+diagonal(d+vec2(2,0),vec2(1,1)));
                let qs=max(EPS*EPS,diagonal(a+vec2(2,0),vec2(-1,1))+diagonal(b,vec2(-1,1))+diagonal(d,vec2(-1,1)));
                pq=ps/(ps+qs);
            }
        }
        if inside(pos,2) && ch!=1u {
            low=v+0.5*(raw(pos+vec2(0,-1))+raw(pos+vec2(0,1))+raw(pos+vec2(-1,0))+raw(pos+vec2(1,0)))+0.25*(raw(pos+vec2(-1,-1))+raw(pos+vec2(1,-1))+raw(pos+vec2(-1,1))+raw(pos+vec2(1,1)));
        }
        maps[i]=vh;maps[n+i]=low;maps[2u*n+i]=pq;return;
    }
    var result=vec3(previous[3u*i],previous[3u*i+1u],previous[3u*i+2u]);
    if stage==1u && inside(pos,4) && ch!=1u {
        let vertical=weighted(green_estimate(pos,vec2(0,-1)),green_estimate(pos,vec2(0,1)));
        let horizontal=weighted(green_estimate(pos,vec2(-1,0)),green_estimate(pos,vec2(1,0)));
        result.y=blend(direction(pos,0u),horizontal,vertical);
    }
    if stage==2u && inside(pos,4) && ch!=1u {
        let other=2u-ch;
        let a=weighted(color_estimate(pos,vec2(1,-1),other),color_estimate(pos,vec2(-1,1),other));
        let b=weighted(color_estimate(pos,vec2(-1,-1),other),color_estimate(pos,vec2(1,1),other));
        result[other]=result.y+blend(direction(pos,2u*n),a,b);
    }
    if stage==3u {
        if inside(pos,10) {
            if ch==1u {
                for(var c=0u;c<=2u;c+=2u) {
                    let horizontal=weighted(color_estimate(pos,vec2(-1,0),c),color_estimate(pos,vec2(1,0),c));
                    let vertical=weighted(color_estimate(pos,vec2(0,-1),c),color_estimate(pos,vec2(0,1),c));
                    result[c]=result.y+blend(direction(pos,0u),horizontal,vertical);
                }
            }
            result=max(result,vec3(0.0));
        } else {result=bilinear(pos);}
    }
    for(var c=0u;c<3u;c++) {output[3u*i+c]=result[c];}
}
