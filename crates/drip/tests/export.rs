//! TIFF export, read back and checked against independently computed values.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use drip::eval::{NodeError, run_action};
use drip::graph::{NodeId, Port};
use drip::node::{Evaluated, NodeKind, OutputSpec, Registry};
use drip::nodes;
use drip::project::Project;
use drip::value::{PortType, Rgb, Value};
use lcms2::{CIExyY, Profile, ToneCurve};
use serde_json::json;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// White, middle grey, Rec.2020 green (outside sRGB), black.
const PIXELS: [[f32; 3]; 4] =
    [[1.0, 1.0, 1.0], [0.18, 0.18, 0.18], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];

static DISPLAY: NodeKind = NodeKind {
    name: "test.display",
    version: 1,
    params: &[],
    inputs: &[],
    outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
    eval: |_, _, _| {
        let image = Rgb { width: 2, height: 2, scale: 1, pixels: PIXELS.to_vec() };
        Ok(Evaluated { outputs: vec![Value::DisplayRec2020(Arc::new(image))], view: None })
    },
    actions: &[],
    migrate: None,
};

fn registry() -> Registry {
    nodes::registry().with(&DISPLAY)
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

fn export(out: &Path, profile: &Path, params: serde_json::Value) -> Result<(), NodeError> {
    let reg = registry();
    let mut p = Project::default();
    let (src, tiff) = (p.graph.add_node(&DISPLAY), p.graph.add_node(&nodes::TIFF));
    p.graph.connect(&reg, Port(src, "image".into()), Port(tiff, "image".into())).unwrap();
    set(&mut p, tiff, json!({ "path": out, "profile": profile }));
    set(&mut p, tiff, params);
    run_action(&p, &reg, tiff, "export")
}

fn set(p: &mut Project, id: NodeId, params: serde_json::Value) {
    for (name, value) in params.as_object().unwrap() {
        p.graph.set_param(&registry(), id, name, value.clone()).unwrap();
    }
}

fn read(path: &Path) -> (DecodingResult, Vec<u8>, u32) {
    let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(decoder.dimensions().unwrap(), (2, 2));
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
    let compression = decoder.get_tag_u32(Tag::Compression).unwrap();
    (decoder.read_image().unwrap(), icc, compression)
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

#[test]
fn rec2020_output_is_an_identity_and_embeds_the_profile() {
    let s = Scratch::new("rec2020");
    let (profile, out) = (s.profile("rec2020.icc", &nodes::rec2020_linear()), s.0.join("out.tif"));
    export(&out, &profile, json!({ "compression": "none" })).unwrap();
    let (DecodingResult::U16(data), icc, compression) = read(&out) else { panic!("not 16 bit") };
    assert_eq!(icc, std::fs::read(&profile).unwrap());
    assert_eq!(compression, 1);
    for (got, want) in data.iter().zip(PIXELS.as_flattened()) {
        assert!((f64::from(*got) - f64::from(*want) * 65535.0).abs() <= 1.0, "{data:?}");
    }
}

#[test]
fn srgb_output_encodes_and_clips_out_of_gamut_colors() {
    let s = Scratch::new("srgb");
    let (profile, out) = (s.profile("srgb.icc", &Profile::new_srgb()), s.0.join("out.tif"));
    export(&out, &profile, json!({})).unwrap();
    let (DecodingResult::U16(data), _, compression) = read(&out) else { panic!("not 16 bit") };
    assert_eq!(compression, 8, "deflate by default");
    let grey = srgb_encode(0.18) * 65535.0;
    let expected = [[65535.0; 3], [grey; 3], [0.0, 65535.0, 0.0], [0.0; 3]];
    for (got, want) in data.iter().zip(expected.as_flattened()) {
        // LittleCMS evaluates the sRGB curve through 16-bit tables.
        assert!((f64::from(*got) - want).abs() <= 40.0, "{data:?}");
    }
}

#[test]
fn float_output_keeps_full_precision() {
    let s = Scratch::new("float");
    let (profile, out) = (s.profile("rec2020.icc", &nodes::rec2020_linear()), s.0.join("out.tif"));
    export(&out, &profile, json!({ "depth": "f32", "deflate_level": "best" })).unwrap();
    let (DecodingResult::F32(data), _, compression) = read(&out) else { panic!("not float") };
    assert_eq!(compression, 8);
    for (got, want) in data.iter().zip(PIXELS.as_flattened()) {
        assert!((got - want).abs() < 1e-5, "{data:?}");
    }
}

#[test]
fn unusable_profiles_are_rejected() {
    let s = Scratch::new("reject");
    let gray =
        Profile::new_gray(&CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 }, &ToneCurve::new(2.2)).unwrap();
    let (gray, out) = (s.profile("gray.icc", &gray), s.0.join("out.tif"));
    let NodeError::Failed(message) = export(&out, &gray, json!({})).unwrap_err() else { panic!() };
    assert!(message.contains("not an RGB output profile"), "{message}");
    let garbage = s.0.join("garbage.icc");
    std::fs::write(&garbage, b"not a profile").unwrap();
    let NodeError::Failed(message) = export(&out, &garbage, json!({})).unwrap_err() else {
        panic!()
    };
    assert!(message.contains("not an ICC profile"), "{message}");
    assert!(!out.exists(), "nothing written on failure");
}
