use drip::{
    definition::Assets,
    eval::Evaluator,
    node::{self, data::*, export::jpeg},
    project::Project,
    registry::{NODES, Registry},
    runtime::RuntimeContext,
};
use exif::{In, Tag, Value};
use std::{io::Cursor, sync::Arc};

fn description(width: u32, height: u32) -> RgbDesc {
    RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65))
}

#[test]
fn jpeg_decodes_pixels_profile_and_capture_tags_with_each_subsampling() {
    let desc = description(17, 11);
    let pixels = vec![[0.18; 3]; desc.extent.pixels()];
    let metadata = drip_raw::Metadata {
        make: "Camera maker".into(),
        model: "Camera model".into(),
        shutter: 1. / 320.,
        aperture: 6.3,
        focal_length: 50.,
        iso: 400.,
        datetime: "2026:10:12 12:34:56".into(),
        orientation: 6,
        ..Default::default()
    };
    for subsampling in
        [jpeg::Subsampling::Full, jpeg::Subsampling::HalfHorizontal, jpeg::Subsampling::Half]
    {
        let p = jpeg::Export { subsampling, ..Default::default() };
        let output = p.load().unwrap();
        let bytes = jpeg::encode(&p, &output, &desc, &pixels, Some(&metadata)).unwrap();
        let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
        let decoded = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        assert_eq!((info.width, info.height), (17, 11));
        assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
        // The sRGB encoding of 18% linear grey is 118, within JPEG rounding.
        assert_eq!(decoded.len(), pixels.len() * 3);
        assert!(decoded.iter().all(|&value| (i16::from(value) - 118).abs() <= 1));
        assert_eq!(decoder.icc_profile().unwrap(), output.icc);

        let exif = exif::Reader::new().read_raw(decoder.exif_data().unwrap().to_vec()).unwrap();
        let field = |tag| &exif.get_field(tag, In::PRIMARY).unwrap().value;
        assert!(matches!(field(Tag::Make), Value::Ascii(v) if v == &[b"Camera maker".to_vec()]));
        assert!(matches!(field(Tag::Model), Value::Ascii(v) if v == &[b"Camera model".to_vec()]));
        assert!(
            matches!(field(Tag::ExposureTime), Value::Rational(v) if v[0].num == 1 && v[0].denom == 320)
        );
        assert!(
            matches!(field(Tag::FNumber), Value::Rational(v) if (v[0].to_f64() - 6.3).abs() < 1e-4)
        );
        assert!(matches!(field(Tag::FocalLength), Value::Rational(v) if v[0].to_f64() == 50.));
        assert_eq!(field(Tag::PhotographicSensitivity).get_uint(0), Some(400));
        assert!(
            matches!(field(Tag::DateTimeOriginal), Value::Ascii(v) if v == &[metadata.datetime.as_bytes().to_vec()])
        );
        assert!(matches!(field(Tag::ExifVersion), Value::Undefined(v, _) if v == b"0232"));
        assert!(exif.get_field(Tag::Orientation, In::PRIMARY).is_none());
    }
}

#[test]
fn chroma_reduction_averages_alternating_colours() {
    let desc = description(32, 16);
    let pixels: Vec<_> = (0..desc.extent.pixels())
        .map(|i| if i % 2 == 0 { [0.4, 0.05, 0.05] } else { [0.05, 0.05, 0.4] })
        .collect();
    let decode = |subsampling| {
        let p = jpeg::Export { quality: 100, subsampling, ..Default::default() };
        let bytes = jpeg::encode(&p, &p.load().unwrap(), &desc, &pixels, None).unwrap();
        jpeg_decoder::Decoder::new(Cursor::new(bytes)).decode().unwrap()
    };
    let full = decode(jpeg::Subsampling::Full);
    for subsampling in [jpeg::Subsampling::HalfHorizontal, jpeg::Subsampling::Half] {
        let reduced = decode(subsampling);
        // Averaging each adjacent red/blue pair retains their mean chroma.
        // Nearest-neighbour subsampling would retain only the red stripes.
        for channel in 0..3 {
            let mean = |data: &[u8]| {
                data.as_chunks::<3>().0.iter().map(|pixel| f32::from(pixel[channel])).sum::<f32>()
                    / pixels.len() as f32
            };
            assert!((mean(&full) - mean(&reduced)).abs() < 4.);
        }
        assert!(reduced.as_chunks::<6>().0.iter().all(|pair| {
            let chroma = |p: &[u8]| i16::from(p[0]) - i16::from(p[2]);
            (chroma(&pair[..3]) - chroma(&pair[3..])).abs() <= 3
        }));
    }
}

#[test]
fn jpeg_validates_parameters_interpretation_and_encoding_boundaries() {
    let p = jpeg::Export::default();
    let output = p.load().unwrap();
    let registry = Registry::new(&[NODES]);
    for quality in [serde_json::json!(0), serde_json::json!(101), serde_json::json!(1.5)] {
        let mut parameters = serde_json::to_value(&p).unwrap();
        parameters["quality"] = quality;
        assert!(registry.build("export-jpeg", parameters).is_err());
    }
    for extent in [(65536, 1), (1, 65536), (0, 1), (1, 0)] {
        assert!(jpeg::encode(&p, &output, &description(extent.0, extent.1), &[], None).is_err());
    }
    assert!(jpeg::encode(&p, &output, &description(2, 1), &[[0.; 3]], None).is_err());
    let mut desc = description(1, 1);
    desc.coordinates = Coordinates::Space(RgbSpace { white: [0.3127, 0.33], ..REC2020_D65 });
    assert!(jpeg::encode(&p, &output, &desc, &[[0.; 3]], None).is_err());

    let bytes =
        jpeg::encode(&p, &output, &description(65535, 1), &vec![[0.; 3]; 65535], None).unwrap();
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    assert!(decoder.decode().unwrap().iter().all(|&sample| sample == 0));
    assert_eq!(decoder.info().unwrap().width, 65535);
    assert!(decoder.exif_data().is_none());
}

#[test]
fn jpeg_action_survives_project_round_trip_and_writes_selected_profile() {
    let path = std::env::temp_dir().join(format!("drip-jpeg-action-{}.jpg", std::process::id()));
    let mut project = Project::default();
    let mut output = node::profile::Settings::default();
    output.profile = node::profile::ProfileSource::DisplayP3;
    let p = jpeg::Export {
        path: Some(path.clone()),
        output,
        quality: 87,
        subsampling: jpeg::Subsampling::HalfHorizontal,
    };
    let expected_profile = p.load().unwrap().icc;
    jpeg::write::add(&mut project.dag, p).unwrap();
    let saved = project.to_json().unwrap();
    let mut restored = Project::from_json(&saved, &Registry::new(&[NODES])).unwrap();
    assert_eq!(restored.to_json().unwrap(), saved);
    let export = jpeg::write::Ports::from_node(restored.dag.nodes().next().unwrap().0);
    let image = node::source::<Rgb>(&mut restored.dag, description(1, 1), vec![[0.18; 3]]).unwrap();
    let metadata = Arc::new(drip_raw::Metadata::default());
    let capture =
        node::source::<CaptureMetadata>(&mut restored.dag, metadata.clone(), metadata).unwrap();
    restored.dag.connect(image, export.image).unwrap();
    restored.dag.connect(capture, export.metadata).unwrap();
    let evaluator = Evaluator::new(RuntimeContext::host());
    let target = evaluator
        .evaluate_targets(&restored.dag, &Default::default(), &[export.node])
        .remove(&export.node)
        .unwrap()
        .unwrap();
    let node = restored.dag.node(export.node).unwrap();
    (node.kind().action.unwrap().run)(node, target.inputs().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.decode().unwrap();
    assert_eq!(decoder.icc_profile().unwrap(), expected_profile);
    let exif = exif::Reader::new().read_raw(decoder.exif_data().unwrap().to_vec()).unwrap();
    assert!(exif.get_field(Tag::Make, In::PRIMARY).is_none());
    assert!(exif.get_field(Tag::ExposureTime, In::PRIMARY).is_none());
}
