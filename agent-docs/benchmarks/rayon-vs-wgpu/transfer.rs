// Diagnose the two host-copy stages of the current download path; no node changes.
use drip::compute::Compute;
use std::time::Instant;
fn main() {
    let compute = Compute::new().unwrap();
    println!("pixels,iteration,readback_ms,rgb_copy_ms");
    for pixels in [1992 * 1330, 3984 * 2660, 7968 * 5320] {
        let data = vec![0.25f32; pixels * 3];
        let gpu = compute.upload_f32(&data).unwrap();
        compute.finish().unwrap();
        drop(data);
        for i in 0..10 {
            let start = Instant::now();
            let flat = compute.read_f32(&gpu).unwrap();
            let readback = start.elapsed().as_secs_f64() * 1000.0;
            let start = Instant::now();
            let rgb = flat.as_chunks::<3>().0.to_vec();
            std::hint::black_box(&rgb);
            let copy = start.elapsed().as_secs_f64() * 1000.0;
            if i > 0 {
                println!("{pixels},{i},{readback:.6},{copy:.6}");
            }
        }
    }
}
