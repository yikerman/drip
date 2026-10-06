use super::*;
use crate::color::{self, D65, REC709, REC2020};
use crate::graph::Graph;
use crate::image::SceneRec2020;
use crate::project::Project;
use serde_json::json;

fn preview(image: &dyn RgbIn<Rec2020>, params: serde_json::Value) -> Result<PreviewImage, String> {
    let mut graph = Graph::default();
    let id = graph.add_node(&PREVIEW);
    for (name, value) in params.as_object().unwrap() {
        graph.set_param(id, name, value.clone()).unwrap();
    }
    let context = EvalContext { level: 0, resources: &Default::default() };
    let result =
        Preview::eval(Params::validated(&graph.node(id).unwrap().params), (image,), &context)?;
    let Some(View::Image(image)) = result.view else { panic!("image view") };
    Ok(image)
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
fn none_reuses_scene_and_display_pixels_without_loading_a_profile() {
    let rgb = image(vec![[-0.2, 0.18, 2.0]]);
    let scene = SceneRec2020::from(rgb.clone());
    let display = DisplayRec2020::from(rgb.clone());
    for input in [&scene as &dyn RgbIn<Rec2020>, &display] {
        let shown = preview(input, json!({ "profile": "file" })).unwrap();
        assert!(Arc::ptr_eq(shown.rgb(), &rgb));
    }
}

#[test]
fn softproof_bounds_the_target_gamut_and_keeps_image_geometry() {
    let rgb = image(vec![[0.0; 3], [0.18; 3], [1.0; 3], [0.0, 1.0, 0.0], [-0.1; 3], [2.0; 3]]);
    let shown = preview(&SceneRec2020::from(rgb.clone()), json!({ "mode": "softproof" })).unwrap();
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
    let input = DisplayRec2020::from(rgb.clone());
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
fn active_modes_report_missing_profiles() {
    let input = DisplayRec2020::from(image(vec![[0.18; 3]]));
    for mode in ["softproof", "gamutcheck"] {
        let error = preview(&input, json!({ "mode": mode, "profile": "file" })).unwrap_err();
        assert_eq!(error, "no output profile file chosen");
    }
}

#[test]
fn proof_settings_round_trip_as_ordinary_node_parameters() {
    let mut project = Project::default();
    let id = project.graph.add_node(&PREVIEW);
    for (name, value) in [
        ("mode", json!("softproof")),
        ("profile", json!("file")),
        ("profile_file", json!("proof.icc")),
        ("intent", json!("absolute")),
        ("black_point_compensation", json!(false)),
    ] {
        project.graph.set_param(id, name, value).unwrap();
    }
    let loaded = Project::from_json(&project.to_json(), &crate::nodes::registry()).unwrap();
    assert_eq!(loaded, project);
}
