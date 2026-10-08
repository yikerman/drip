// Flat RGB storage has 12 bytes per pixel; WGSL vec3 storage would pad to 16.
@group(0) @binding(0) var<storage, read> source: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<storage, read> p: array<f32>;

@compute @workgroup_size(256)
fn exposure(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 256u;
    if i < arrayLength(&source) { output[i] = source[i] * p[0]; }
}

@compute @workgroup_size(256)
fn matrix(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = (id.x + id.y * groups.x * 256u) * 3u;
    if i + 2u >= arrayLength(&source) { return; }
    let v = vec3(source[i], source[i+1u], source[i+2u]);
    for(var c=0u; c<3u; c++) {
        output[i+c] = p[c*3u]*v.x + p[c*3u+1u]*v.y + p[c*3u+2u]*v.z;
    }
}

@compute @workgroup_size(256)
fn white_balance(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i=id.x+id.y*groups.x*256u;
    if i >= arrayLength(&source) { return; }
    let width=u32(p[0]); let size=u32(p[1]);
    let phase=(i/width%size)*size+i%width%size;
    output[i]=source[i]*p[2u+phase];
}

// Parameters: input width/height, CFA size, RGB channel at each CFA site.
@compute @workgroup_size(256)
fn bin2x2(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i=id.x+id.y*groups.x*256u; let width=u32(p[0]); let height=u32(p[1]); let size=u32(p[2]);
    let ow=width/2u;
    if i >= ow*(height/2u) { return; }
    let row=i/ow*2u; let col=i%ow*2u;
    var sum=vec3(0.0); var count=vec3(0.0);
    for(var r=0u;r<2u;r++) { for(var c=0u;c<2u;c++) {
        let channel=u32(p[3u+((row+r)%size)*size+(col+c)%size]);
        sum[channel]+=source[(row+r)*width+col+c]; count[channel]+=1.0;
    }}
    for(var c=0u;c<3u;c++) { output[3u*i+c]=sum[c]/count[c]; }
}

@compute @workgroup_size(256)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i=id.x+id.y*groups.x*256u; let width=u32(p[0]); let height=u32(p[1]); let ow=width/4u*2u;
    if i >= ow*(height/4u*2u) { return; }
    let row=i/ow; let col=i%ow;
    let r=row/2u*4u+row%2u; let c=col/2u*4u+col%2u;
    let j=r*width+c;
    output[i]=(source[j]+source[j+2u]+source[j+2u*width]+source[j+2u*width+2u])*0.25;
}
