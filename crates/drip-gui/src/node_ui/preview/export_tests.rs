//! Proofing agrees with independently decoded 16-bit exports.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use drip::eval::{Evaluator, run_action};
use drip::graph::{NodeId, Port};
use drip::image::{ColorImage, Rgb};
use drip::node::{NodeDeclaration, NodeKind};
use drip::project::Project;
use drip::{nodes, profile};
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde_json::json;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

const PIXELS: [[f32; 3]; 4] = [[1.0; 3], [0.18; 3], [0.0, 1.0, 0.0], [0.0; 3]];

static DISPLAY: NodeKind =
    NodeKind::new::<Display>("test.proof-source", "test", "display", &[], &["image"]);
struct Display;
impl NodeDeclaration for Display {
    type Parameters = ();
    type Inputs = ();
    type Outputs = (Arc<ColorImage>,);
    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|(), (), _| {
        let image = Rgb { width: 2, height: 2, scale: 1, pixels: PIXELS.to_vec() };
        Ok((Arc::new(ColorImage::from(Arc::new(image))),))
    });
}

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
fn softproof_matches_bounded_export_across_profiles_and_intents() {
    let scratch = Scratch::new("proof");
    let custom = scratch.profile("linear.icc", &profile::rec2020_linear());
    let path = scratch.0.join("proof.tif");
    let mut project = Project::default();
    let source = project.graph.add_node(&DISPLAY);
    let preview = project.graph.add_node(&nodes::PREVIEW);
    let tiff = project.graph.add_node(&nodes::TIFF);
    for node in [preview, tiff] {
        project.graph.connect(Port(source, "image".into()), Port(node, "image".into())).unwrap();
    }
    set(&mut project, tiff, json!({ "path": path }));
    let mut evaluator = Evaluator::default();
    set(&mut project, preview, json!({ "mode": "softproof" }));

    for target in ["srgb", "display_p3", "rec2020", "file"] {
        for intent in ["perceptual", "relative", "saturation", "absolute"] {
            for bpc in [false, true] {
                for node in [preview, tiff] {
                    set(
                        &mut project,
                        node,
                        json!({
                            "profile": target, "profile_file": custom,
                            "intent": intent, "black_point_compensation": bpc,
                        }),
                    );
                }
                let shown = evaluator
                    .with_inputs(&project.graph, preview, 0, &nodes::PREVIEW, super::prepare)
                    .unwrap();

                run_action(&project.graph, tiff, "export").unwrap();
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
