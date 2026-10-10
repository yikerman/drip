//! Proofing agrees with independently decoded 16-bit exports.

use std::path::{Path, PathBuf};

use crate::model::{NodeId, Port};
use drip::{
    eval::Evaluator,
    node::data::{Color, ColorRgb, Extent, ImageDesc},
    ports::{Cpu, Write},
    runtime::{KernelContext, RuntimeContext},
};

use crate::model::Project;
use crate::model::Registry;
use drip::node::profile;
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde_json::json;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

const PIXELS: [[f32; 3]; 4] = [[1.0; 3], [0.18; 3], [0.0, 1.0, 0.0], [0.0; 3]];

fn source_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
) -> drip::Result<(Option<ImageDesc<Color>>,)> {
    Ok((Some(ImageDesc {
        extent: Extent { width: 2, height: 2 },
        interpretation: drip::node::raw::working_color(),
    }),))
}
#[drip::node(id="test.proof-source", name="display", category="test", contract=source_contract)]
fn display(_: &KernelContext<'_>, _: &(), image: Write<'_, Cpu<ColorRgb>>) -> drip::Result<()> {
    image.data.copy_from_slice(&PIXELS);
    Ok(())
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
    let source = project.graph.add_node(Registry.get("test.proof-source").unwrap()).unwrap();
    let preview = project.graph.add_node(Registry.get("view.preview").unwrap()).unwrap();
    let tiff = project.graph.add_node(Registry.get("output.tiff").unwrap()).unwrap();
    for node in [preview, tiff] {
        project.graph.connect(Port(source, "image".into()), Port(node, "image".into())).unwrap();
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
                let params =
                    serde_json::from_value(project.graph.node(preview).unwrap().params).unwrap();
                let shown =
                    super::prepare(params, &ctx.image::<Color>(&values).unwrap(), &ctx).unwrap();
                let values = inputs.remove(&tiff).unwrap().unwrap();
                let node = project.graph.node(tiff).unwrap();
                crate::node_ui::binding(node.kind)
                    .unwrap()
                    .run_action("Export", node.params, &values, &ctx)
                    .unwrap();
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
