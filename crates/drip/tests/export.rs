//! TIFF export, read back and checked against independently computed values.

use drip::image::{DisplayRec2020, RawMetadata};
use drip::node::{EvalContext, KernelError, NodeKernel};
use drip::param::Params;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use drip::color::{self, D65, P3, REC2020};
use drip::eval::{Evaluator, NodeError, run_action};
use drip::graph::{NodeId, Port};
use drip::image::Rgb;
use drip::node::{Evaluated, NodeKind};
use drip::nodes;
use drip::profile;
use drip::project::Project;
use drip::view::View;
use lcms2::{CIExyY, InfoType, Intent, Locale, PixelFormat, Profile, ToneCurve, Transform};
use serde_json::json;
use tiff::decoder::ifd::Value;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// White, middle grey, Rec.2020 green (outside sRGB), black.
const PIXELS: [[f32; 3]; 4] =
    [[1.0, 1.0, 1.0], [0.18, 0.18, 0.18], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];

static DISPLAY: NodeKind =
    NodeKind::new::<DisplayKernel>("test.display", "test", "display", &[], &[], &["image"]);
struct DisplayKernel;
impl NodeKernel for DisplayKernel {
    type Inputs = ();
    type Outputs = (Arc<DisplayRec2020>,);

    fn eval(
        _: Params<'_>,
        (): (),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, KernelError> {
        let image = Rgb { width: 2, height: 2, scale: 1, pixels: PIXELS.to_vec() };
        Ok(Evaluated { outputs: (Arc::new(DisplayRec2020::from(Arc::new(image))),), view: None })
    }
}

static METADATA: NodeKind =
    NodeKind::new::<MetadataKernel>("test.metadata", "test", "metadata", &[], &[], &["metadata"]);
struct MetadataKernel;
impl NodeKernel for MetadataKernel {
    type Inputs = ();
    type Outputs = (Arc<RawMetadata>,);

    fn eval(
        _: Params<'_>,
        (): (),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, KernelError> {
        Ok(Evaluated::new((Arc::new(RawMetadata {
            make: "Sony".into(),
            model: "ILCE-7RM3".into(),
            iso: 100.0,
            shutter: 1.0 / 320.0,
            aperture: 6.3,
            focal_length: 0.0,
            timestamp: 0,
            datetime: "2026:05:25 08:35:00".into(),
        }),)))
    }
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

fn export(out: &Path, params: serde_json::Value) -> Result<(), NodeError> {
    let mut p = Project::default();
    let (src, tiff) = (p.graph.add_node(&DISPLAY), p.graph.add_node(&nodes::TIFF));
    p.graph.connect(Port(src, "image".into()), Port(tiff, "image".into())).unwrap();
    set(&mut p, tiff, json!({ "path": out }));
    set(&mut p, tiff, params);
    run_action(&p.graph, tiff, "export")
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

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// Checks 16-bit output against `expected` (0 to 1) within `tolerance` codes.
fn assert_u16(data: &[u16], expected: [[f64; 3]; 4], tolerance: f64) {
    for (got, want) in data.iter().zip(expected.as_flattened()) {
        assert!((f64::from(*got) - want * 65535.0).abs() <= tolerance, "{data:?} vs {expected:?}");
    }
}

fn description(icc: &[u8]) -> String {
    Profile::new_icc(icc).unwrap().info(InfoType::Description, Locale::none()).unwrap()
}

#[test]
fn a_linear_rec2020_profile_file_is_an_identity_and_is_embedded() {
    let s = Scratch::new("rec2020");
    let (profile, out) = (s.profile("linear.icc", &profile::rec2020_linear()), s.0.join("out.tif"));
    export(&out, json!({ "profile": "file", "profile_file": profile, "compression": "none" }))
        .unwrap();
    let (DecodingResult::U16(data), icc, compression) = read(&out) else { panic!("not 16 bit") };
    assert_eq!(icc, std::fs::read(&profile).unwrap());
    assert_eq!(compression, 1);
    assert_u16(&data, PIXELS.map(|p| p.map(f64::from)), 1.0);
}

#[test]
fn built_in_srgb_encodes_and_clips_out_of_gamut_colors() {
    let s = Scratch::new("srgb");
    let out = s.0.join("out.tif");
    export(&out, json!({})).unwrap();
    let (DecodingResult::U16(data), icc, compression) = read(&out) else { panic!("not 16 bit") };
    assert_eq!((compression, description(&icc).as_str()), (8, "sRGB"), "deflated sRGB by default");
    let grey = srgb_encode(0.18);
    // LittleCMS evaluates curves through 16-bit tables.
    assert_u16(&data, [[1.0; 3], [grey; 3], [0.0, 1.0, 0.0], [0.0; 3]], 40.0);
}

#[test]
fn built_in_display_p3_matches_its_primaries() {
    let s = Scratch::new("p3");
    let out = s.0.join("out.tif");
    export(&out, json!({ "profile": "display_p3" })).unwrap();
    let (DecodingResult::U16(data), icc, _) = read(&out) else { panic!("not 16 bit") };
    assert_eq!(description(&icc), "Display P3");
    // Rec.2020 green in P3 coordinates, clipped to the P3 gamut.
    let to_p3 =
        color::mul(&color::inverse(&color::rgb_to_xyz(P3, D65)), &color::rgb_to_xyz(REC2020, D65));
    let green = color::apply(&to_p3, [0.0, 1.0, 0.0]).map(|v| srgb_encode(v.clamp(0.0, 1.0)));
    assert_u16(&data, [[1.0; 3], [srgb_encode(0.18); 3], green, [0.0; 3]], 40.0);
}

#[test]
fn built_in_rec2020_uses_the_bt2020_curve() {
    let s = Scratch::new("bt2020");
    let out = s.0.join("out.tif");
    export(&out, json!({ "profile": "rec2020" })).unwrap();
    let (DecodingResult::U16(data), icc, _) = read(&out) else { panic!("not 16 bit") };
    assert_eq!(description(&icc), "Rec. 2020");
    let grey = 1.099 * 0.18f64.powf(0.45) - 0.099;
    assert_u16(&data, [[1.0; 3], [grey; 3], [0.0, 1.0, 0.0], [0.0; 3]], 40.0);
}

#[test]
fn float_output_keeps_full_precision() {
    let s = Scratch::new("float");
    let (profile, out) = (s.profile("linear.icc", &profile::rec2020_linear()), s.0.join("out.tif"));
    let params = json!({ "profile": "file", "profile_file": profile, "depth": "f32", "deflate_level": "best" });
    export(&out, params).unwrap();
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
    let failure = |params| match export(&out, params).unwrap_err() {
        NodeError::Failed(message) => message,
        e => panic!("{e}"),
    };
    let message = failure(json!({ "profile": "file", "profile_file": gray }));
    assert!(message.contains("not an RGB output profile"), "{message}");
    let garbage = s.0.join("garbage.icc");
    std::fs::write(&garbage, b"not a profile").unwrap();
    let message = failure(json!({ "profile": "file", "profile_file": garbage }));
    assert!(message.contains("not an ICC profile"), "{message}");
    assert_eq!(
        export(&out, json!({ "profile": "file" })),
        Err(NodeError::Incomplete("no output profile file chosen"))
    );
    assert!(!out.exists(), "nothing written on failure");
}

#[test]
fn an_export_without_a_destination_is_incomplete() {
    let mut p = Project::default();
    let (src, tiff) = (p.graph.add_node(&DISPLAY), p.graph.add_node(&nodes::TIFF));
    p.graph.connect(Port(src, "image".into()), Port(tiff, "image".into())).unwrap();
    assert_eq!(
        run_action(&p.graph, tiff, "export"),
        Err(NodeError::Incomplete("no output file chosen"))
    );
}

#[test]
fn softproof_matches_bounded_export_and_only_recomputes_the_preview() {
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
    evaluator.evaluate(&project.graph, 0, &[preview]);
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
                assert_eq!(evaluator.evaluate(&project.graph, 0, &[preview]), [preview]);
                let result = evaluator.result(preview).unwrap().as_ref().unwrap();
                let Some(View::Image(shown)) = &result.view else { panic!("image") };

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

#[test]
fn connected_camera_metadata_is_written_as_exif() {
    let s = Scratch::new("exif");
    let out = s.0.join("out.tif");
    let mut p = Project::default();
    let g = &mut p.graph;
    let (src, metadata, tiff) =
        (g.add_node(&DISPLAY), g.add_node(&METADATA), g.add_node(&nodes::TIFF));
    g.connect(Port(src, "image".into()), Port(tiff, "image".into())).unwrap();
    set(&mut p, tiff, json!({ "path": out }));
    run_action(&p.graph, tiff, "export").unwrap();
    let mut decoder = Decoder::new(std::fs::File::open(&out).unwrap()).unwrap();
    assert!(decoder.find_tag(Tag::ExifDirectory).unwrap().is_none(), "metadata is optional");

    p.graph.connect(Port(metadata, "metadata".into()), Port(tiff, "metadata".into())).unwrap();
    run_action(&p.graph, tiff, "export").unwrap();
    let mut decoder = Decoder::new(std::fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(decoder.get_tag_ascii_string(Tag::Make).unwrap(), "Sony");
    assert_eq!(decoder.get_tag_ascii_string(Tag::Model).unwrap(), "ILCE-7RM3");
    let exif = decoder.get_tag(Tag::ExifDirectory).unwrap().into_ifd_pointer().unwrap();
    let exif = decoder.read_directory(exif).unwrap();
    let tags: Vec<_> = decoder
        .read_directory_tags(&exif)
        .tag_iter()
        .map(|tag| tag.map(|(tag, value)| (tag.to_u16(), value)).unwrap())
        .collect();
    let datetime = Value::Ascii("2026:05:25 08:35:00".into());
    let expected = [
        (0x829a, Value::Rational(1, 320)),
        (0x829d, Value::Rational(63, 10)),
        (0x8827, Value::Short(100)),
        (0x9000, Value::List(b"0232".map(Value::Byte).to_vec())),
        (0x9003, datetime),
    ];
    assert_eq!(tags, expected, "unknown focal length omitted");
    assert!(matches!(decoder.read_image().unwrap(), DecodingResult::U16(_)));
}
