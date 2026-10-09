use super::*;
use drip::node::color::{self, D65, REC709, REC2020};
type Rec2020Mat = Arc<Rgb>;

use crate::model::Project;
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde_json::json;

fn preview(image: &Rec2020Mat, params: serde_json::Value) -> Result<PreviewImage, KernelError> {
    let mut values = serde_json::to_value(Preview::default()).unwrap();
    for (name, value) in params.as_object().unwrap() {
        values[name] = value.clone();
    }
    let params: Preview = serde_json::from_value(values).unwrap();
    super::prepare(params, image, &PrepareContext::default())
}

fn image(pixels: Vec<[f32; 3]>) -> Arc<Rgb> {
    Arc::new(Rgb { width: pixels.len(), height: 1, requested_scale: 4, pixels: pixels.into() })
}

fn close(actual: [f32; 3], expected: [f32; 3]) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 0.0002, "{actual:?} vs {expected:?}");
    }
}

#[test]
fn none_reuses_linear_rec2020_pixels_without_loading_a_profile() {
    let rgb = image(vec![[-0.2, 0.18, 2.0]]);
    let input = Rec2020Mat::from(rgb.clone());
    {
        let input = &input;
        let shown = preview(input, json!({ "profile": "file" })).unwrap();
        assert!(Arc::ptr_eq(shown.rgb(), &rgb));
        assert!(!shown.interpolation);
        let interpolated = preview(input, json!({ "interpolation": true })).unwrap();
        assert!(interpolated.interpolation);
        assert!(Arc::ptr_eq(interpolated.rgb(), &rgb));
    }
}

#[test]
fn softproof_bounds_the_target_gamut_and_keeps_image_geometry() {
    let rgb = image(vec![[0.0; 3], [0.18; 3], [1.0; 3], [0.0, 1.0, 0.0], [-0.1; 3], [2.0; 3]]);
    let shown = preview(&Rec2020Mat::from(rgb.clone()), json!({ "mode": "softproof" })).unwrap();
    let shown = shown.rgb();
    assert_eq!(
        (shown.width, shown.height, shown.requested_scale),
        (rgb.width, rgb.height, rgb.requested_scale)
    );
    let rec_to_srgb = color::mul(
        &color::inverse(&color::rgb_to_xyz(REC709, D65)),
        &color::rgb_to_xyz(REC2020, D65),
    );
    let srgb_to_rec = color::inverse(&rec_to_srgb);
    for (&input, &got) in rgb.pixels.iter().zip(shown.pixels.iter()) {
        let bounded = color::apply(&rec_to_srgb, input.map(f64::from)).map(|v| v.clamp(0.0, 1.0));
        close(got, color::apply(&srgb_to_rec, bounded).map(|v| v as f32));
    }
    assert_eq!(rgb.pixels[3], [0.0, 1.0, 0.0], "source stays unchanged");
}

#[test]
fn gamutcheck_marks_colors_against_the_selected_profile() {
    let rgb = image(vec![[0.0; 3], [0.18; 3], [1.0; 3], [0.0, 1.0, 0.0]]);
    let input = Rec2020Mat::from(rgb.clone());
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
    let shown = preview(&Rec2020Mat::from(rgb.clone()), json!({"mode":"gamutcheck"})).unwrap();
    for (&got, &want) in shown.rgb().pixels.iter().zip(rgb.pixels.iter()) {
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
    let input = Rec2020Mat::from(image(pixels));
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
    let shown = preview(&Rec2020Mat::from(rgb.clone()), json!({"mode":"gamutcheck"})).unwrap();
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
    let input = Rec2020Mat::from(image(vec![[0.18; 3]]));
    for mode in ["softproof", "gamutcheck"] {
        let error = preview(&input, json!({ "mode": mode, "profile": "file" })).unwrap_err();
        assert_eq!(error, KernelError::Contract("no output profile file chosen".into()));
    }
}

#[test]
fn proof_settings_round_trip_as_ordinary_node_parameters() {
    let mut project = Project::default();
    let id = project.graph.add_node(crate::model::Registry.get("view.preview").unwrap()).unwrap();
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
    let loaded = Project::from_json(&project.to_json().unwrap(), &crate::model::Registry).unwrap();
    assert_eq!(loaded.to_json().unwrap(), project.to_json().unwrap());
}
