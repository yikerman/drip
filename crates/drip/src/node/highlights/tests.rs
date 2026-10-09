use super::*;
use crate::{payload::Payload, runtime::RuntimeContext};

const PHASES: [BayerPhase; 4] =
    [BayerPhase::Rggb, BayerPhase::Grbg, BayerPhase::Gbrg, BayerPhase::Bggr];

fn evaluate(
    runtime: &RuntimeContext,
    width: usize,
    height: usize,
    phase: BayerPhase,
    clips: [f32; 4],
    threshold: f32,
    samples: &[f32],
) -> Vec<f32> {
    let desc = BayerDesc {
        extent: Extent { width: width as u32, height: height as u32 },
        phase,
        interpretation: Camera {
            coordinates: "opposed-reference".into(),
            scale: "balanced".into(),
        },
    };
    let levels_desc = BayerLevelDesc { phase, interpretation: desc.interpretation.clone() };
    let levels = std::array::from_fn(|i| clips[reference::color(phase, i / 2, i % 2)]);
    let input = Bayer::upload(&desc, &samples.to_vec(), runtime).unwrap();
    let mut output = Bayer::allocate_device(&desc, runtime).unwrap();
    let ctx = KernelContext { global: &GlobalContext::default(), runtime };
    reconstruct(
        &ctx,
        &Settings { threshold },
        Read { desc: &desc, data: &input },
        Read { desc: &levels_desc, data: &levels },
        Write { desc: &desc, data: &mut output },
    )
    .unwrap();
    Bayer::download(&desc, &output, runtime).unwrap()
}
fn fixture(width: usize, height: usize, phase: BayerPhase, clips: [f32; 4]) -> Vec<f32> {
    (0..width * height)
        .map(|i| {
            let (r, c) = (i / width, i % width);
            let ch = reference::color(phase, r, c);
            let value = if (32..58).contains(&r) && (32..64).contains(&c) {
                if ch == 0 { 1.0 } else { 0.9 }
            } else {
                0.35 + ((r * 13 + c * 7) % 31) as f32 / 100.0
            };
            value * clips[ch]
        })
        .collect()
}
fn compare_reference(
    runtime: &RuntimeContext,
    width: usize,
    height: usize,
    phase: BayerPhase,
    clips: [f32; 4],
    threshold: f32,
    samples: &[f32],
) -> Vec<f32> {
    let out = evaluate(runtime, width, height, phase, clips, threshold, samples);
    let mut expected = vec![0.0; samples.len()];
    reference::process(
        &reference::Mosaic { width, height, phase, clips, samples },
        threshold,
        &mut expected,
    );
    let mut max_error = 0.0f32;
    for (i, (&actual, &expected)) in out.iter().zip(&expected).enumerate() {
        let error = (actual - expected).abs();
        max_error = max_error.max(error);
        assert!(
            error < 2e-5 * expected.abs().max(1.0),
            "{phase:?} {width}x{height} [{i}]: {actual} != {expected}"
        );
        let ch = reference::color(phase, i / width, i % width);
        if samples[i] < clips[ch] * threshold {
            assert_eq!(actual.to_bits(), samples[i].to_bits());
        } else {
            assert!(actual >= samples[i]);
        }
    }
    eprintln!("opposed {phase:?} {width}x{height}: max error={max_error}");
    out
}
fn algorithms(runtime: RuntimeContext) {
    let clips = [1.3, 1.0, 1.7, 1.0];
    for (phase_index, phase) in PHASES.into_iter().enumerate() {
        chrominance_count_boundary(&runtime, phase);
        let samples = fixture(96, 90, phase, clips);
        let out = compare_reference(&runtime, 96, 90, phase, clips, 0.98, &samples);
        for line in include_str!("../../../tests/reference/opposed.csv")
            .lines()
            .filter(|s| !s.starts_with('#'))
        {
            let v: Vec<f32> = line.split(',').map(|s| s.parse().unwrap()).collect();
            if v[0] as usize == phase_index {
                let actual = out[v[1] as usize * 96 + v[2] as usize];
                assert!((actual - v[3]).abs() < 2e-5, "upstream {line}: {actual}");
            }
        }
        // Odd extents exercise partial mask cells. 277x269 needs two global
        // reduction passes after producing more than 256 sample-block partials.
        for (w, h) in [(1, 1), (1, 5), (5, 1), (2, 2), (5, 7), (97, 91), (277, 269)] {
            let mut samples = fixture(w, h, phase, clips);
            for i in (0..samples.len()).step_by(7) {
                samples[i] = 3.0;
            }
            compare_reference(&runtime, w, h, phase, clips, 0.98, &samples);
        }
        // Vary opposite channels around clipped red sites so the learned offset
        // and reconstruction change many samples, rather than hitting max(input).
        let samples: Vec<_> = (0..277 * 269)
            .map(|i| {
                if reference::color(phase, i / 277, i % 277) == 0 {
                    if i % 7 == 0 { 1.3 } else { 1.2 }
                } else {
                    0.2 + ((i * 137 + i / 277 * 29) % 1091) as f32 / 300.0
                }
            })
            .collect();
        let out =
            compare_reference(&runtime, 277, 269, phase, [1.3, 4.0, 4.0, 4.0], 0.98, &samples);
        assert!(out.iter().zip(&samples).filter(|(after, before)| after > before).count() > 100);
        // Unequal green-site levels must select samples by local position.
        let clips = [1.1, 0.8, 1.5, 1.2];
        let mut samples = fixture(97, 91, phase, clips);
        for (i, value) in samples.iter_mut().enumerate() {
            let ch = reference::color(phase, i / 97, i % 97);
            let level = clips[ch] * 0.98;
            *value = match i % 11 {
                0 => f32::from_bits(level.to_bits() - 1),
                1 => level,
                2 => f32::from_bits(level.to_bits() + 1),
                3 => -0.2,
                4 => -0.0,
                _ => *value,
            };
        }
        compare_reference(&runtime, 97, 91, phase, clips, 0.98, &samples);
        let samples = fixture(20, 20, phase, clips);
        assert_eq!(evaluate(&runtime, 20, 20, phase, clips, 1.0, &samples), samples);
        let samples: Vec<_> = (0..256)
            .map(|i| if reference::color(phase, i / 16, i % 16) == 0 { 1.0 } else { 2.0 })
            .collect();
        let out = evaluate(&runtime, 16, 16, phase, [1.0, 4.0, 4.0, 4.0], 0.98, &samples);
        assert!(out.iter().all(|v| (*v - 2.0).abs() < 2e-6));
    }
}
fn chrominance_count_boundary(runtime: &RuntimeContext, phase: BayerPhase) {
    let w = 32;
    let seeds: Vec<_> = [12, 18]
        .into_iter()
        .map(|r| {
            (0..4)
                .map(|i| (r + i / 2) * w + r + i % 2)
                .find(|&i| reference::color(phase, i / w, i % w) == 0)
                .unwrap()
        })
        .collect();
    for eligible in [100, 101] {
        let mut samples: Vec<_> = (0..w * w)
            .map(|i| if reference::color(phase, i / w, i % w) == 0 { 0.0 } else { 2.0 })
            .collect();
        for &i in &seeds {
            samples[i] = 1.0;
        }
        let mut count = 0;
        for (i, value) in samples.iter_mut().enumerate() {
            if count < eligible
                && *value == 0.0
                && seeds.iter().any(|&seed| {
                    let dy = (i / w / 3).abs_diff(seed / w / 3);
                    let dx = (i % w / 3).abs_diff(seed % w / 3);
                    dy <= 3 && dx <= 3 && !(dy == 3 && dx == 3)
                })
            {
                *value = 0.9;
                count += 1;
            }
        }
        assert_eq!(count, eligible);
        let out = compare_reference(runtime, w, w, phase, [1.0, 4.0, 4.0, 4.0], 1.0, &samples);
        let expected = if eligible == 100 { 2.0 } else { 1.0 };
        for &i in &seeds {
            assert!(
                (out[i] - expected).abs() < 2e-6,
                "{eligible} samples: {} != {expected}",
                out[i]
            );
        }
    }
}
#[test]
#[ignore = "requires a compute-capable wgpu device"]
fn wgpu_opposed() {
    algorithms(RuntimeContext::wgpu());
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_opposed() {
    algorithms(RuntimeContext::cpu());
}
