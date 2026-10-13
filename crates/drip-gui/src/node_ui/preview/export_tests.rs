//! Proofing agrees with independently decoded 16-bit exports.

use std::path::{Path, PathBuf};

use crate::model::NodeId;
use crate::test_nodes::display;
use drip::{definition::Typed, eval::Evaluator, runtime::RuntimeContext};

use crate::model::Project;
use drip::node::profile;
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde_json::json;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("drip-export-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn profile(&self, name: &str, profile: &Profile) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, profile.icc().unwrap()).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn set(p: &mut Project, id: NodeId, params: serde_json::Value) {
    for (name, value) in params.as_object().unwrap() {
        p.graph.set_param(id, name, value.clone()).unwrap();
    }
}

fn read(path: &Path) -> (DecodingResult, Vec<u8>, u32) {
    let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(decoder.dimensions().unwrap(), (2, 2));
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
    let compression = decoder.get_tag_u32(Tag::Compression).unwrap();
    (decoder.read_image().unwrap(), icc, compression)
}

#[test]
fn export_depth_survives_reopening_and_encodes_expected_samples() {
    let scratch = Scratch::new("depth");
    let path = scratch.0.join("depth.tif");
    let mut project = Project::default();
    let source = project.graph.add_node(&display::KIND).unwrap();
    let export = project.graph.add_node(&drip::node::export::tiff::KIND).unwrap();
    project
        .graph
        .connect(
            project.graph.dag.output_port(source, "image").unwrap(),
            project.graph.dag.input_port(export, "image").unwrap(),
        )
        .unwrap();
    assert_eq!(project.graph.node(export).unwrap().params["depth"], "u16");
    set(&mut project, export, json!({"path":path}));
    let evaluator = Evaluator::new(RuntimeContext::host());
    let expected_profile = profile::Output::load(&Default::default()).unwrap();
    let profile_path = scratch.0.join("srgb.icc");
    std::fs::write(&profile_path, &expected_profile.icc).unwrap();
    set(&mut project, export, json!({"profile":{"file":{"path":profile_path}}}));
    for depth in ["u8", "u16", "f32"] {
        for compression in [json!("none"), json!({"deflate":{"level":"balanced"}})] {
            set(&mut project, export, json!({"depth":depth,"compression":compression}));
            let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
            let export = reopened.graph.find("Export TIFF").unwrap();
            assert_eq!(reopened.graph.node(export).unwrap().params["depth"], depth);
            let values = evaluator
                .evaluate_inputs(&reopened.graph.dag, &Default::default(), &[export])
                .remove(&export)
                .unwrap()
                .unwrap();
            let node = reopened.graph.dag.node(export).unwrap();
            let export = crate::node_ui::action(node.kind()).unwrap();
            export.run(node, &values, &Default::default()).unwrap();
            let (data, icc, compression_tag) = read(&path);
            assert_eq!(icc, expected_profile.icc);
            assert_eq!(compression_tag, if compression == "none" { 1 } else { 8 });
            let (samples, max, tolerance): (Vec<f64>, f64, f64) = match (depth, data) {
                ("u8", DecodingResult::U8(v)) => (v.into_iter().map(f64::from).collect(), 255., 1.),
                ("u16", DecodingResult::U16(v)) => {
                    (v.into_iter().map(f64::from).collect(), 65535., 2.)
                }
                ("f32", DecodingResult::F32(v)) => {
                    (v.into_iter().map(f64::from).collect(), 1., 3e-5)
                }
                _ => panic!("wrong TIFF sample type for {depth}"),
            };
            assert_eq!(samples.len(), 12);
            // Neutral 18% linear light is sRGB-encoded before quantization.
            let grey = 1.055 * 0.18f64.powf(1. / 2.4) - 0.055;
            for (range, expected) in [(0..3, max), (3..6, grey * max), (9..12, 0.)] {
                for &sample in &samples[range] {
                    assert!(
                        (sample - expected).abs() <= tolerance,
                        "{depth}: {sample} vs {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn softproof_matches_bounded_export_across_profiles_and_intents() {
    let scratch = Scratch::new("proof");
    let custom = scratch.profile("linear.icc", &profile::rec2020_linear());
    let path = scratch.0.join("proof.tif");
    let mut project = Project::default();
    let source = project.graph.add_node(&display::KIND).unwrap();
    let preview = project.graph.add_node(&crate::node_ui::preview::observe::KIND).unwrap();
    let tiff = project.graph.add_node(&drip::node::export::tiff::KIND).unwrap();
    for node in [preview, tiff] {
        project
            .graph
            .connect(
                project.graph.dag.output_port(source, "image").unwrap(),
                project.graph.dag.input_port(node, "image").unwrap(),
            )
            .unwrap();
    }
    set(&mut project, tiff, json!({ "path": path }));
    let evaluator = Evaluator::new(RuntimeContext::host());

    for target in ["srgb", "display_p3", "rec2020", "file"] {
        for intent in ["perceptual", "relative", "saturation", "absolute"] {
            for bpc in [false, true] {
                let settings = json!({
                    "profile": if target == "file" { json!({"file": {"path": custom}}) } else { json!(target) },
                    "intent": intent, "black_point_compensation": bpc,
                });
                set(&mut project, tiff, settings.clone());
                set(&mut project, preview, json!({"mode": {"softproof": settings}}));
                let mut inputs = evaluator.evaluate_inputs(
                    &project.graph.dag,
                    &Default::default(),
                    &[preview, tiff],
                );
                let ctx = crate::node_ui::data::PrepareContext::default();
                let values = inputs.remove(&preview).unwrap().unwrap();
                let node = project.graph.dag.node(preview).unwrap();
                let node = <super::observe::Definition as Typed>::of(node).unwrap();
                let bound = values.bind::<super::observe::Definition>().unwrap();
                let image = std::sync::Arc::new(
                    crate::node_ui::data::HostImage::new(&bound.image, ctx.global.scale).unwrap(),
                );
                let output = node.assets().unwrap();
                let shown = super::prepare(&node.parameters, output.as_ref(), &image).unwrap();
                let values = inputs.remove(&tiff).unwrap().unwrap();
                let node = project.graph.dag.node(tiff).unwrap();
                crate::node_ui::action(node.kind()).unwrap().run(node, &values, &ctx).unwrap();
                let (data, icc, _) = read(&path);
                let DecodingResult::U16(data) = data else { panic!("16-bit export") };
                let encoded = data.as_chunks::<3>().0;
                let display = Transform::new(
                    &Profile::new_icc(&icc).unwrap(),
                    PixelFormat::RGB_16,
                    &profile::rec2020_linear(),
                    PixelFormat::RGB_FLT,
                    Intent::RelativeColorimetric,
                )
                .unwrap();
                let mut expected = vec![[0.0f32; 3]; encoded.len()];
                display.transform_pixels(encoded, &mut expected);
                for (&got, &want) in
                    shown.rgb().pixels.as_flattened().iter().zip(expected.as_flattened())
                {
                    assert!(
                        (got - want).abs() < 0.0002,
                        "{target}, {intent}, BPC {bpc}: {got} vs {want}"
                    );
                }
            }
        }
    }
}
