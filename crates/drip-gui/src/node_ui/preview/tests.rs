use super::*;
use drip::color::{self, D65, REC709, REC2020};
use drip::graph::Graph;
use drip::image::ColorImage;
use drip::nodes::preview::PREVIEW;
use drip::project::Project;
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde_json::json;

fn compute() -> &'static Arc<drip::compute::Compute> {
    static COMPUTE: std::sync::OnceLock<Arc<drip::compute::Compute>> = std::sync::OnceLock::new();
    COMPUTE.get_or_init(|| {
        drip::compute::Compute::new().expect("hardware or software adapter required")
    })
}

fn preview(image: &ColorImage, params: serde_json::Value) -> Result<PreviewImage, KernelError> {
    let mut graph = Graph::default();
    let id = graph.add_node(&PREVIEW);
    for (name, value) in params.as_object().unwrap() {
        graph.set_param(id, name, value.clone()).unwrap();
    }
    let resources = drip::resource::Resources::with_compute(compute().clone());
    let context = EvalContext::new(0, &resources).unwrap();
    let image = image.upload(context.compute()?)?;
    let params = <Preview as drip::param::Parameters>::read(drip::param::Params::validated(
        &graph.node(id).unwrap().params,
    ));
    super::prepare(params, (&image,), &context)
}

fn image(pixels: Vec<[f32; 3]>) -> Arc<Rgb> {
    Arc::new(Rgb { width: pixels.len(), height: 1, scale: 4, pixels })
}

fn close(actual: [f32; 3], expected: [f32; 3]) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 0.0002, "{actual:?} vs {expected:?}");
    }
}

#[test]
fn none_keeps_resident_samples_without_loading_a_profile_or_readback() {
    let compute = drip::compute::Compute::new().unwrap();
    let resources = drip::resource::Resources::with_compute(compute.clone());
    let context = EvalContext::new(0, &resources).unwrap();
    let rgb = image(vec![[-0.2, 0.18, 2.0]]);
    let input = ColorImage::from(rgb.clone()).upload(&compute).unwrap();
    let mut graph = Graph::default();
    let id = graph.add_node(&PREVIEW);
    graph.set_param(id, "profile", json!("file")).unwrap();
    for interpolation in [false, true] {
        graph.set_param(id, "interpolation", json!(interpolation)).unwrap();
        let params = <Preview as drip::param::Parameters>::read(drip::param::Params::validated(
            &graph.node(id).unwrap().params,
        ));
        let before = compute.transfer_counts();
        let shown = super::prepare(params, (&input,), &context).unwrap();
        assert_eq!(compute.transfer_counts(), before, "normal preview must not transfer samples");
        assert_eq!(shown.interpolation, interpolation);
        assert_eq!(shown.gpu().unwrap().gpu_buffer().raw(), input.gpu_buffer().raw());
    }
}

#[test]
fn softproof_bounds_the_target_gamut_and_keeps_image_geometry() {
    let rgb = image(vec![[0.0; 3], [0.18; 3], [1.0; 3], [0.0, 1.0, 0.0], [-0.1; 3], [2.0; 3]]);
    let shown = preview(&ColorImage::from(rgb.clone()), json!({ "mode": "softproof" })).unwrap();
    let shown = shown.rgb();
    assert_eq!((shown.width, shown.height, shown.scale), (rgb.width, rgb.height, rgb.scale));
    let rec_to_srgb = color::mul(
        &color::inverse(&color::rgb_to_xyz(REC709, D65)),
        &color::rgb_to_xyz(REC2020, D65),
    );
    let srgb_to_rec = color::inverse(&rec_to_srgb);
    for (&input, &got) in rgb.pixels.iter().zip(&shown.pixels) {
        let bounded = color::apply(&rec_to_srgb, input.map(f64::from)).map(|v| v.clamp(0.0, 1.0));
        close(got, color::apply(&srgb_to_rec, bounded).map(|v| v as f32));
    }
    assert_eq!(rgb.pixels[3], [0.0, 1.0, 0.0], "source stays unchanged");
}

#[test]
fn gamutcheck_marks_colors_against_the_selected_profile() {
    let rgb = image(vec![[0.0; 3], [0.18; 3], [1.0; 3], [0.0, 1.0, 0.0]]);
    let input = ColorImage::from(rgb.clone());
    for target in ["srgb", "display_p3", "rec2020"] {
        let shown = preview(&input, json!({ "mode": "gamutcheck", "profile": target })).unwrap();
        for i in 0..3 {
            close(shown.rgb().pixels[i], rgb.pixels[i]);
        }
        let expected = if target == "rec2020" { rgb.pixels[3] } else { [0.0, 1.0, 1.0] };
        close(shown.rgb().pixels[3], expected);
    }
}

#[test]
fn gamutcheck_preserves_dark_neutrals_and_interior_colors() {
    let mut pixels: Vec<_> = [-6, -5, -4, -3, -2, -1, 0].map(|ev| [10.0f32.powi(ev); 3]).into();
    pixels.extend([[0.002, 0.003, 0.002], [0.02, 0.03, 0.02], [0.2, 0.3, 0.2]]);
    let rgb = image(pixels);
    let shown = preview(&ColorImage::from(rgb.clone()), json!({"mode":"gamutcheck"})).unwrap();
    for (&got, &want) in shown.rgb().pixels.iter().zip(&rgb.pixels) {
        close(got, want);
    }
}

#[test]
fn proofing_is_identical_across_worker_counts_and_chunk_boundaries() {
    let pixels = (0..32771)
        .map(|i| match i % 4 {
            0 => [0.001; 3],
            1 => [0.0, 1.0, 0.0],
            2 => [0.18; 3],
            _ => [0.2, 0.3, 0.2],
        })
        .collect();
    let input = ColorImage::from(image(pixels));
    let run = |workers, mode| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| preview(&input, json!({"mode":mode})).unwrap())
    };
    for mode in ["softproof", "gamutcheck"] {
        assert_eq!(run(1, mode), run(4, mode));
    }
}

#[test]
fn gamut_warnings_agree_with_lcms_round_trips_away_from_the_boundary() {
    use lcms2::CIELabExt;
    let lab = Profile::new_lab4_context(lcms2::GlobalContext::new(), lcms2::CIExyY::d50()).unwrap();
    let target = profile::ProfileSource::Srgb.built_in().unwrap();
    let working = profile::rec2020_linear();
    let to_lab = Transform::new(
        &working,
        PixelFormat::RGB_FLT,
        &lab,
        PixelFormat::Lab_DBL,
        Intent::RelativeColorimetric,
    )
    .unwrap();
    let forward = Transform::new(
        &lab,
        PixelFormat::Lab_DBL,
        &target,
        PixelFormat::RGB_16,
        Intent::RelativeColorimetric,
    )
    .unwrap();
    let reverse = Transform::new(
        &target,
        PixelFormat::RGB_16,
        &lab,
        PixelFormat::Lab_DBL,
        Intent::RelativeColorimetric,
    )
    .unwrap();
    // Move from an interior color through the sRGB green boundary. LCMS's
    // GamutSampler [15] compares two bounded device round trips in Lab.
    let pixels = (0..101)
        .map(|i| {
            let t = i as f32 / 100.0;
            [0.18 * (1.0 - t), 0.18 + 0.82 * t, 0.18 * (1.0 - t)]
        })
        .collect();
    let rgb = image(pixels);
    let shown = preview(&ColorImage::from(rgb.clone()), json!({"mode":"gamutcheck"})).unwrap();
    let mut original = vec![[0.0f64; 3]; rgb.pixels.len()];
    to_lab.transform_pixels(&rgb.pixels, &mut original);
    let mut device = vec![[0u16; 3]; original.len()];
    let mut first = original.clone();
    let mut second = original.clone();
    forward.transform_pixels(&original, &mut device);
    reverse.transform_pixels(&device, &mut first);
    forward.transform_pixels(&first, &mut device);
    reverse.transform_pixels(&device, &mut second);
    let delta = |a: &[f64; 3], b: &[f64; 3]| {
        let lab = |&[l, a, b]: &[f64; 3]| lcms2::CIELab { L: l, a, b };
        lab(a).delta_e(&lab(b))
    };
    let losses: Vec<_> = original.iter().zip(&first).map(|(a, b)| delta(a, b)).collect();
    // For matrix profiles LCMS subtracts 1 delta E and rounds to an integer
    // warning. Its 49-point Lab grid has a cell diagonal just under 8 delta E.
    let boundary = original[losses.iter().position(|&loss| loss >= 1.5).unwrap()];
    let mut inside = 0;
    let mut outside = 0;
    for (i, &loss) in losses.iter().enumerate() {
        assert!(delta(&first[i], &second[i]) < 0.05, "stable device round trip");
        if delta(&original[i], &boundary) <= 8.0 {
            continue;
        }
        let marked = shown.rgb().pixels[i] == [0.0, 1.0, 1.0];
        assert_eq!(marked, loss >= 1.5, "delta E {loss}, Lab {:?}", original[i]);
        if marked {
            outside += 1;
        } else {
            inside += 1;
        }
    }
    assert!(inside > 10 && outside > 10);
}

#[test]
fn active_modes_report_missing_profiles() {
    let input = ColorImage::from(image(vec![[0.18; 3]]));
    for mode in ["softproof", "gamutcheck"] {
        let error = preview(&input, json!({ "mode": mode, "profile": "file" })).unwrap_err();
        assert_eq!(error, KernelError::Incomplete("no output profile file chosen"));
    }
}

#[test]
fn proof_settings_round_trip_as_ordinary_node_parameters() {
    let mut project = Project::default();
    let id = project.graph.add_node(&PREVIEW);
    for (name, value) in [
        ("interpolation", json!(true)),
        ("mode", json!("softproof")),
        ("profile", json!("file")),
        ("profile_file", json!("proof.icc")),
        ("intent", json!("absolute")),
        ("black_point_compensation", json!(false)),
    ] {
        project.graph.set_param(id, name, value).unwrap();
    }
    let loaded = Project::from_json(&project.to_json(), &drip::nodes::registry()).unwrap();
    assert_eq!(loaded, project);
}

#[test]
fn preview_rejects_nonadditive_coordinates_at_the_boundary() {
    let input = ColorImage::try_new(
        image(vec![[0.5, 0.0, 0.0]]),
        drip::image::ColorMeaning {
            coordinates: drip::image::ColorCoordinates::Oklab,
            scene: drip::image::SceneRelationship::Unspecified,
        },
    )
    .unwrap();
    assert!(preview(&input, json!({})).unwrap_err().to_string().contains("additive"));
}
