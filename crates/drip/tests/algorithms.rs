use drip::{eval::Evaluator, graph::Dag, node, node::data::*, ports::*, runtime::RuntimeContext};

fn algorithms(runtime: RuntimeContext) {
    let descriptor = ImageDesc {
        extent: Extent { width: 2, height: 1 },
        interpretation: node::raw::working_color(),
    };
    let bits = [[0x80000000, 0x7fc01234, 0x7f800000], [1, 0xff800000, 0x3f800000]];
    let pixels: Vec<[f32; 3]> = bits.map(|p| p.map(f32::from_bits)).to_vec();
    let device = ColorRgb::upload(&descriptor, &pixels, &runtime).unwrap();
    let restored = ColorRgb::download(&descriptor, &device, &runtime).unwrap();
    assert_eq!(restored.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(), bits);

    let evaluator = Evaluator::new(runtime);
    bayer_reduction(&evaluator);
    resident_highlights(&evaluator);
    context_driven_demosaic(&evaluator);
    let phases = [BayerPhase::Rggb, BayerPhase::Grbg, BayerPhase::Gbrg, BayerPhase::Bggr];
    let camera = Camera { coordinates: "reference-camera".into(), scale: "relative".into() };
    for (phase_index, phase) in phases.into_iter().enumerate() {
        let (w, h) = (277, 269);
        let mut dag = Dag::new();
        let source = node::source::<Bayer>(
            &mut dag,
            BayerDesc {
                extent: Extent { width: w, height: h },
                phase,
                interpretation: camera.clone(),
            },
            (0..w * h).map(|i| ((i * 137 + i / w * 29) % 2048) as f32 / 1024. - 0.03).collect(),
        )
        .unwrap();
        let rcd = node::rcd::add(&mut dag, ()).unwrap();
        dag.connect(source, rcd.image).unwrap();
        let output =
            evaluator.evaluate::<Cpu<CameraRgb>>(&dag, &Default::default(), rcd.output).unwrap();
        let mut max_error = 0f32;
        let mut sum = 0.;
        let mut count = 0;
        for line in include_str!("reference/rcd.csv").lines().filter(|l| !l.starts_with('#')) {
            let v: Vec<f32> = line.split(',').map(|s| s.parse().unwrap()).collect();
            if v[0] as usize != phase_index {
                continue;
            }
            let pixel = output.data[v[1] as usize * w as usize + v[2] as usize];
            for c in 0..3 {
                let error = (pixel[c] - v[3 + c]).abs();
                max_error = max_error.max(error);
                sum += error;
                count += 1;
            }
        }
        // Direction ties are sensitive to f32 reassociation across backends.
        // Bound both outliers and average error against independent upstream C.
        eprintln!("RCD phase {phase_index}: max={max_error}, mean={}", sum / count as f32);
        assert!(max_error < 0.005 && sum / (count as f32) < 0.0002);
    }
    for (w, h) in [(1, 1), (2, 3), (11, 9)] {
        let mut dag = Dag::new();
        let source = node::source::<Bayer>(
            &mut dag,
            BayerDesc {
                extent: Extent { width: w, height: h },
                phase: BayerPhase::Rggb,
                interpretation: camera.clone(),
            },
            vec![0.5; (w * h) as usize],
        )
        .unwrap();
        let n = node::rcd::add(&mut dag, ()).unwrap();
        dag.connect(source, n.image).unwrap();
        let out =
            evaluator.evaluate::<Cpu<CameraRgb>>(&dag, &Default::default(), n.output).unwrap();
        assert!(out.data.iter().flatten().all(|v| v.is_finite()));
    }
    let mut dag = Dag::new();
    let source = node::source::<ColorRgb>(
        &mut dag,
        ImageDesc {
            extent: Extent { width: 5, height: 1 },
            interpretation: node::raw::working_color(),
        },
        vec![[0.; 3], [0.18; 3], [1.; 3], [f32::MAX; 3], [-0.2, 0.1, 0.5]],
    )
    .unwrap();
    let n = node::sigmoid::apply::add(&mut dag, Default::default()).unwrap();
    dag.connect(source, n.image).unwrap();
    let out = evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &Default::default(), n.output).unwrap();
    assert!(out.data.iter().flatten().all(|v| v.is_finite()));
    for v in out.data[1] {
        assert!((v - 0.18).abs() < 1e-5);
    }
    for v in out.data[3] {
        assert!((v - 1.).abs() < 1e-5);
    }
    assert_eq!(out.data[0], [0.; 3]);
}
#[test]
#[ignore = "requires a compute-capable wgpu device"]
fn wgpu_algorithms() {
    algorithms(RuntimeContext::wgpu());
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_algorithms() {
    algorithms(RuntimeContext::cpu());
}

fn bayer_reduction(evaluator: &Evaluator) {
    let camera = Camera { coordinates: "reduction-test".into(), scale: "white-1".into() };
    for phase in [BayerPhase::Rggb, BayerPhase::Grbg, BayerPhase::Gbrg, BayerPhase::Bggr] {
        for (width, height, factor) in [(8, 12, 2), (7, 9, 2), (3, 3, 256), (3, 3, 1), (1, 3, 1)] {
            let mut dag = Dag::new();
            let pixels: Vec<_> = (0..width * height)
                .map(|i| ((i / width) % 2 * 2 + (i % width) % 2) as f32 + 1.)
                .collect();
            let source = node::source::<Bayer>(
                &mut dag,
                BayerDesc {
                    extent: Extent { width, height },
                    phase,
                    interpretation: camera.clone(),
                },
                pixels.clone(),
            )
            .unwrap();
            let reduced =
                node::reduce::bayer::add(&mut dag, node::reduce::Settings { factor }).unwrap();
            dag.connect(source, reduced.image).unwrap();
            let result = evaluator
                .evaluate::<Cpu<Bayer>>(&dag, &Default::default(), reduced.output)
                .unwrap();
            assert_eq!(result.desc.phase, phase);
            for (i, &value) in result.data.iter().enumerate() {
                let w = result.desc.extent.width as usize;
                assert_eq!(value, ((i / w) % 2 * 2 + (i % w) % 2) as f32 + 1.);
            }
            if factor == 1 {
                assert_eq!(*result.data, pixels);
            }
        }
    }
    for (pixels, expected) in [
        ((0..16).map(|i| i as f32).collect(), vec![5., 6., 9., 10.]),
        (vec![f32::MAX; 16], vec![f32::MAX; 4]),
        (vec![-f32::MAX; 16], vec![-f32::MAX; 4]),
        ((0..16).map(|i| if i < 8 { f32::MAX } else { -f32::MAX }).collect(), vec![0.; 4]),
    ] {
        let mut dag = Dag::new();
        let source = node::source::<Bayer>(
            &mut dag,
            BayerDesc {
                extent: Extent { width: 4, height: 4 },
                phase: BayerPhase::Rggb,
                interpretation: camera.clone(),
            },
            pixels,
        )
        .unwrap();
        let reduced = node::reduce::bayer::add(&mut dag, Default::default()).unwrap();
        dag.connect(source, reduced.image).unwrap();
        let result =
            evaluator.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), reduced.output).unwrap();
        for (actual, expected) in result.data.iter().zip(expected) {
            assert!(actual.is_finite());
            assert!(
                (*actual - expected).abs() <= expected.abs().max(1.) * 1e-6,
                "{actual} != {expected}"
            );
        }
    }
}

fn context_driven_demosaic(evaluator: &Evaluator) {
    use drip::runtime::GlobalContext;
    let camera = Camera { coordinates: "context-camera".into(), scale: "white-1".into() };
    let mut dag = Dag::new();
    let source = node::source::<Bayer>(
        &mut dag,
        BayerDesc {
            extent: Extent { width: 64, height: 48 },
            phase: BayerPhase::Grbg,
            interpretation: camera.clone(),
        },
        (0..64 * 48).map(|i| ((i * 31 + i / 64) % 101) as f32 / 100.).collect(),
    )
    .unwrap();
    let demosaic = node::rcd::add(&mut dag, ()).unwrap();
    dag.connect(source, demosaic.image).unwrap();
    let matrix = node::source::<Matrix3<Camera, Color>>(
        &mut dag,
        MatrixDesc { source: camera, target: node::raw::working_color() },
        [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    )
    .unwrap();
    let convert = node::camera_to_rgb::add(&mut dag, ()).unwrap();
    dag.connect(demosaic.output, convert.image).unwrap();
    dag.connect(matrix, convert.matrix).unwrap();
    let preview = node::bayer_preview::add(&mut dag, ()).unwrap();
    dag.connect(source, preview.image).unwrap();
    let node_count = dag.nodes().count();
    let edges: Vec<_> = dag.edges().collect();
    let global = GlobalContext { scale: 2 };
    let image = evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &global, convert.output).unwrap();
    assert_eq!(image.desc.extent, Extent { width: 32, height: 24 });
    assert_eq!(image.data.len(), 32 * 24);
    assert_eq!(image.statistics.nodes, 4); // No inserted reduction node.
    assert_eq!(image.statistics.uploads, 2);
    assert_eq!(image.statistics.downloads, 1);
    assert_eq!(dag.nodes().count(), node_count);
    assert_eq!(dag.edges().collect::<Vec<_>>(), edges);
    assert_eq!(
        dag.description(convert.output).unwrap().unwrap().extent,
        Extent { width: 64, height: 48 }
    );
    let binned = evaluator.evaluate::<Cpu<CameraRgb>>(&dag, &global, preview.output).unwrap();
    assert_eq!(binned.desc.extent, Extent { width: 16, height: 12 });
    // An explicit reduction followed by full-detail demosaic is the reference.
    let mut explicit = dag.clone();
    let reduced =
        node::reduce::bayer::add(&mut explicit, node::reduce::Settings { factor: 2 }).unwrap();
    explicit.connect(source, reduced.image).unwrap();
    explicit
        .edit(|edit| {
            edit.disconnect(demosaic.image.id())?;
            edit.connect(reduced.output, demosaic.image)?;
            edit.disconnect(preview.image.id())?;
            edit.connect(reduced.output, preview.image)
        })
        .unwrap();
    let expected = evaluator
        .evaluate::<Cpu<ColorRgb>>(&explicit, &Default::default(), convert.output)
        .unwrap();
    for (a, b) in image.data.iter().flatten().zip(expected.data.iter().flatten()) {
        assert!((a - b).abs() < 2e-6);
    }
    let expected = evaluator
        .evaluate::<Cpu<CameraRgb>>(&explicit, &Default::default(), preview.output)
        .unwrap();
    assert_eq!(*binned.data, *expected.data);
    let full =
        evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &Default::default(), convert.output).unwrap();
    assert_eq!(full.desc.extent, Extent { width: 64, height: 48 });
}

fn resident_highlights(evaluator: &Evaluator) {
    let camera = Camera { coordinates: "highlight-test".into(), scale: "balanced".into() };
    let mut dag = Dag::new();
    let source = node::source::<Bayer>(
        &mut dag,
        BayerDesc {
            extent: Extent { width: 32, height: 24 },
            phase: BayerPhase::Rggb,
            interpretation: camera.clone(),
        },
        vec![0.5; 32 * 24],
    )
    .unwrap();
    let levels = node::source::<BayerLevels>(
        &mut dag,
        BayerLevelDesc { phase: BayerPhase::Rggb, interpretation: camera },
        [1.; 4],
    )
    .unwrap();
    let before = node::reduce::bayer::add(&mut dag, node::reduce::Settings { factor: 1 }).unwrap();
    let highlights = node::highlights::reconstruct::add(&mut dag, Default::default()).unwrap();
    let after = node::bayer_preview::add(&mut dag, ()).unwrap();
    dag.connect(source, before.image).unwrap();
    dag.connect(before.output, highlights.image).unwrap();
    dag.connect(levels, highlights.levels).unwrap();
    dag.connect(highlights.output, after.image).unwrap();
    let output =
        evaluator.evaluate::<Cpu<CameraRgb>>(&dag, &Default::default(), after.output).unwrap();
    assert!(output.data.iter().flatten().all(|v| (*v - 0.5).abs() < 1e-6));
    // Highlights must keep the image on the device between its two neighbors.
    assert_eq!(output.statistics.uploads, 1);
    assert_eq!(output.statistics.downloads, 1);
}
