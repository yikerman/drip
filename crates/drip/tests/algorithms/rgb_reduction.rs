use drip::{eval::Evaluator, graph::Dag, node, node::data::*, ports::*};

pub fn check(evaluator: &Evaluator) {
    for (width, height, factor) in
        [(5, 3, 2), (1, 7, 3), (9, 1, 4), (3, 2, 256), (3, 2, 1), (257, 259, 256)]
    {
        let pixels = (0..width * height)
            .map(|i| [i as f32 / 128., -(i as f32) / 256., ((i * 31) % 101) as f32 / 100.])
            .collect();
        check_case(evaluator, width, height, factor, pixels);
    }
    check_case(evaluator, 256, 256, 256, vec![[f32::MAX, -f32::MAX, 1.]; 256 * 256]);
    check_case(
        evaluator,
        4,
        4,
        4,
        (0..16).map(|i| [if i < 8 { f32::MAX } else { -f32::MAX }, 0., -2.]).collect(),
    );
    check_case(
        evaluator,
        2,
        1,
        2,
        vec![[f32::NAN, f32::INFINITY, f32::NEG_INFINITY], [f32::MAX; 3]],
    );
    resident_chain(evaluator);
}

fn check_case(evaluator: &Evaluator, width: u32, height: u32, factor: i64, pixels: Vec<[f32; 3]>) {
    let extent = Extent { width, height };
    // f32 cancellation error is relative to input magnitude, not a near-zero mean.
    let magnitude = std::array::from_fn::<_, 3, _>(|c| {
        pixels.iter().map(|p| p[c].abs()).filter(|v| v.is_finite()).fold(1.0f32, f32::max)
    });
    let expected = reference(&pixels, width as usize, height as usize, factor as usize);
    let output_extent =
        Extent { width: width.div_ceil(factor as u32), height: height.div_ceil(factor as u32) };
    let camera = Camera { coordinates: "reduction-camera".into(), scale: "native".into() };
    // Box reduction averages declared samples; it does not require linear encoding.
    let color = Color { encoding: Encoding::Srgb, ..node::raw::working_color() };
    let mut dag = Dag::new();
    let camera_source = node::source::<CameraRgb>(
        &mut dag,
        ImageDesc { extent: extent.clone(), interpretation: camera.clone() },
        pixels.clone(),
    )
    .unwrap();
    let color_source = node::source::<ColorRgb>(
        &mut dag,
        ImageDesc { extent, interpretation: color.clone() },
        pixels,
    )
    .unwrap();
    let camera_node =
        node::reduce::camera::add(&mut dag, node::reduce::Settings { factor }).unwrap();
    let color_node = node::reduce::color::add(&mut dag, node::reduce::Settings { factor }).unwrap();
    dag.connect(camera_source, camera_node.image).unwrap();
    dag.connect(color_source, color_node.image).unwrap();
    // Explicit reduction uses its parameter, not the request's demosaic scale.
    let context = drip::runtime::GlobalContext { scale: 4 };
    let a = evaluator.evaluate::<Cpu<CameraRgb>>(&dag, &context, camera_node.output).unwrap();
    let b = evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &context, color_node.output).unwrap();
    assert_eq!(a.desc.extent, output_extent);
    assert_eq!(b.desc.extent, output_extent);
    assert_eq!(a.desc.interpretation, camera);
    assert_eq!(b.desc.interpretation, color);
    for result in [&*a.data, &*b.data] {
        assert_eq!(result.len(), expected.len());
        for (i, (&actual, &expected)) in
            result.iter().flatten().zip(expected.iter().flatten()).enumerate()
        {
            if expected.is_nan() {
                assert!(actual.is_nan());
            } else if expected.is_infinite() {
                assert_eq!(actual, expected);
            } else {
                assert!(actual.is_finite());
                assert!(
                    (actual - expected).abs() <= magnitude[i % 3] * 2e-5,
                    "{width}x{height}, factor {factor}: {actual} != {expected}"
                );
            }
        }
    }
}

// Independent f64 reference retains the former CPU arithmetic for comparison.
fn reference(pixels: &[[f32; 3]], width: usize, height: usize, factor: usize) -> Vec<[f32; 3]> {
    let mut result = Vec::new();
    for top in (0..height).step_by(factor) {
        for left in (0..width).step_by(factor) {
            let mut sum = [0.; 3];
            let mut count = 0;
            for row in pixels.chunks_exact(width).skip(top).take(factor) {
                for pixel in row.iter().skip(left).take(factor) {
                    for c in 0..3 {
                        sum[c] += f64::from(pixel[c]);
                    }
                    count += 1;
                }
            }
            result.push(sum.map(|v| (v / count as f64) as f32));
        }
    }
    result
}

fn resident_chain(evaluator: &Evaluator) {
    let mut dag = Dag::new();
    let source = node::source::<ColorRgb>(
        &mut dag,
        ImageDesc {
            extent: Extent { width: 5, height: 3 },
            interpretation: node::raw::working_color(),
        },
        vec![[0.25; 3]; 15],
    )
    .unwrap();
    let before = node::exposure::add(&mut dag, node::Exposure { ev: 1. }).unwrap();
    let reduced = node::reduce::color::add(&mut dag, Default::default()).unwrap();
    let after = node::exposure::add(&mut dag, node::Exposure { ev: -1. }).unwrap();
    dag.connect(source, before.image).unwrap();
    dag.connect(before.output, reduced.image).unwrap();
    dag.connect(reduced.output, after.image).unwrap();
    let result =
        evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &Default::default(), after.output).unwrap();
    assert_eq!(result.statistics.uploads, 1);
    assert_eq!(result.statistics.downloads, 1);
    assert_eq!(&**result.data, &[[0.25; 3]; 6]);
}
