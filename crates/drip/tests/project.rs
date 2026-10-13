use drip::registry::{NODES, Registry};
use drip::{
    eval::Evaluator, node, node::data::*, project::Project, runtime::RuntimeContext, templates,
};
use serde_json::json;

fn registry() -> Registry {
    Registry::new(&[NODES])
}

fn raw_pipeline() -> drip::Result<(drip::graph::Dag, templates::Chain)> {
    let mut dag = drip::graph::Dag::new();
    let chain = templates::neutral_restore(&mut dag)?;
    Ok((dag, chain))
}

#[test]
fn project_round_trip_uses_named_ports_and_revalidates_parameters() {
    let mut p = Project::default();
    let first = node::sigmoid::apply::add(&mut p.dag, Default::default()).unwrap();
    let apply = node::sigmoid::apply::add(&mut p.dag, Default::default()).unwrap();
    p.dag.connect(first.output, apply.image).unwrap();
    p.ui = json!({"retained":true});
    p.node_ui.insert(apply.node, json!({"pos":[12,34]}));
    let serialized = p.to_json().unwrap();
    let restored = Project::from_json(&serialized, &registry()).unwrap();
    assert_eq!(restored.dag.edges().count(), 1);
    assert_eq!(restored.ui, p.ui);
    assert_eq!(restored.to_json().unwrap(), serialized);
    let (first, _) = restored.dag.nodes().next().unwrap();
    let output = restored.dag.output_port(first, "output").unwrap();
    let output = restored.dag.typed_output::<Rgb>(output).unwrap();
    // An incomplete image pipeline loads; evaluation reports what is missing.
    let eval = Evaluator::new(RuntimeContext::host());
    let error = eval.evaluate::<Rgb>(&restored.dag, &Default::default(), output).err();
    assert!(matches!(error, Some(drip::Error::MissingInput { port: "image", .. })));
    let mut data: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    data["nodes"][0]["parameters"]["contrast"] = json!(-1.);
    assert!(Project::from_json(&data.to_string(), &registry()).is_err());
    data = serde_json::from_str(&serialized).unwrap();
    data["edges"][0][0]["port"] = json!("missing");
    assert!(Project::from_json(&data.to_string(), &registry()).is_err());
    data = serde_json::from_str(&serialized).unwrap();
    let major: u64 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
    assert_eq!(data["version"], major);
    data["version"] = json!(major + 1);
    let error = Project::from_json(&data.to_string(), &registry()).err().unwrap().to_string();
    assert!(error.contains("not migrated"), "{error}");
}
#[test]
fn export_reads_its_profile_file_when_first_used() {
    use drip::definition::Typed;
    use drip::node::export::tiff::Definition;
    let file = std::env::temp_dir().join(format!("drip-profile-{}.icc", std::process::id()));
    let srgb = drip::node::profile::Output::load(&Default::default()).unwrap().icc;
    std::fs::write(&file, &srgb).unwrap();
    let mut params = serde_json::to_value(drip::node::export::Export::default()).unwrap();
    params["profile"] = json!({"file": {"path": file}});
    let loaded = registry().build("export-tiff", params.clone()).unwrap();
    let icc = || Definition::of(&*loaded).unwrap().assets().map(|output| output.icc.clone());
    assert_eq!(icc().unwrap(), srgb);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(icc().unwrap(), srgb, "files are assumed unchanged once read");
    // A missing file is the node's error, not a refused edit.
    let missing = registry().build("export-tiff", params.clone()).unwrap();
    let error = Definition::of(&*missing).unwrap().assets().err().unwrap();
    assert!(error.to_string().contains(&*file.to_string_lossy()), "{error}");
    assert_eq!(missing.parameters().unwrap(), params);
}

#[test]
fn tiff_profile_extent_and_pixels_survive_encoding() {
    let desc = RgbDesc::new(Extent { width: 2, height: 1 }, Coordinates::Space(REC2020_D65));
    let export = drip::node::export::Export::default();
    let output = drip::definition::Assets::load(&export).unwrap();
    let encode = |desc, metadata| {
        drip::node::export::encode(&export, &output, desc, &[[0.; 3], [1.; 3]], metadata)
    };
    let metadata = drip_raw::Metadata { orientation: 6, ..Default::default() };
    let bytes = encode(&desc, Some(&metadata)).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(decoder.dimensions().unwrap(), (2, 1));
    // Geometry has already determined stored pixel order. The capture tag must
    // not ask another viewer to rotate the exported result a second time.
    assert!(decoder.get_tag(tiff::tags::Tag::Orientation).is_err());
    assert!(!decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap().is_empty());
    let tiff::decoder::DecodingResult::U16(pixels) = decoder.read_image().unwrap() else {
        panic!("expected u16")
    };
    assert_eq!(&pixels[..3], &[0, 0, 0]);
    assert!(pixels[3..].iter().all(|&v| v > 65530));
    let mut bad = desc.clone();
    bad.coordinates = Coordinates::Space(RgbSpace { white: [0.3127, 0.33], ..REC2020_D65 });
    assert!(encode(&bad, None).is_err());
}

#[test]
fn missing_raw_preserves_editable_parameters_and_reports_load_error() {
    let path = std::path::Path::new("/tmp/drip-deliberately-missing-raw-source.nef");
    assert!(!path.exists());
    let (mut dag, _) = raw_pipeline().unwrap();
    let raw = dag.nodes().find(|(_, n)| n.kind().id == "bayer-raw").unwrap().0;
    dag.set_parameters(raw, json!({ "path": path })).unwrap();
    let text = Project { dag, ..Default::default() }.to_json().unwrap();
    let mut restored = Project::from_json(&text, &registry()).unwrap();
    assert_eq!(restored.to_json().unwrap(), text);
    let sigmoid = restored.dag.nodes().find(|(_, n)| n.kind().id == "sigmoid").unwrap().0;
    let target = restored.dag.output_port(sigmoid, "output").unwrap();
    let target = restored.dag.typed_output::<Rgb>(target).unwrap();
    let error = Evaluator::new(RuntimeContext::host())
        .evaluate::<Rgb>(&restored.dag, &Default::default(), target)
        .err()
        .unwrap();
    assert!(error.to_string().contains("drip-deliberately-missing-raw-source.nef"));
    let raw = restored.dag.nodes().find(|(_, n)| n.kind().id == "bayer-raw").unwrap().0;
    let values = Evaluator::new(RuntimeContext::host()).evaluate_targets(
        &restored.dag,
        &Default::default(),
        &[target.node()],
    );
    assert!(matches!(&values[&target.node()], Err(drip::Error::Node { node, source, .. })
        if *node == raw && source.to_string().contains("drip-deliberately-missing-raw-source.nef")));
    restored.dag.set_parameters(raw, json!({"path":null})).unwrap();
    assert!(restored.dag.node(raw).unwrap().parameters().unwrap()["path"].is_null());
}

/// The store keeps a decode under the file's stamp: the same file is a hit,
/// a file with another modification time decodes again, and a removed file
/// fails evaluation.
#[test]
fn raw_is_decoded_once_per_file_stamp() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/raw/pixls/nikon-d70s.nef");
    let temporary =
        std::env::temp_dir().join(format!("drip-raw-snapshot-{}.nef", std::process::id()));
    std::fs::copy(&fixture, &temporary).unwrap();
    let mut dag = drip::graph::Dag::new();
    let source = dag
        .add(node::raw::definition(node::raw::Settings { path: Some(temporary.clone()) }).unwrap())
        .unwrap();
    let output = dag.typed_output::<Bayer>(dag.output_port(source, "mosaic").unwrap()).unwrap();
    let evaluator = Evaluator::new(RuntimeContext::host());
    let evaluate =
        |dag: &drip::graph::Dag| evaluator.evaluate::<Bayer>(dag, &Default::default(), output);
    let first = evaluate(&dag).unwrap();
    assert_eq!(first.statistics.nodes, 1);
    // Every graph snapshot sharing the file shares the stored decode.
    let again = evaluate(&dag.clone()).unwrap();
    assert_eq!(again.statistics.nodes, 0);
    assert_eq!(first.data.host().unwrap().as_ptr(), again.data.host().unwrap().as_ptr());
    let earlier = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1 << 30);
    std::fs::File::options().write(true).open(&temporary).unwrap().set_modified(earlier).unwrap();
    let touched = evaluate(&dag).unwrap();
    assert_eq!(touched.statistics.nodes, 1);
    assert_eq!(touched.data.host().unwrap(), first.data.host().unwrap());
    std::fs::remove_file(&temporary).unwrap();
    let error = evaluate(&dag).err().unwrap();
    assert!(error.to_string().contains(&*temporary.to_string_lossy()), "{error}");
}

#[test]
fn edits_and_project_open_do_not_depend_on_loaded_sources() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/raw/pixls/nikon-d70s.nef");
    let raw = json!({"path":fixture});
    for load_first in [false, true] {
        let (mut dag, t) = raw_pipeline().unwrap();
        if load_first {
            dag.set_parameters(t.raw.node, raw.clone()).unwrap();
        }
        // Skipping color conversion leaves Sigmoid, after Orientation and
        // Exposure, with camera RGB; only its evaluation reports that.
        dag.edit(|e| {
            e.disconnect(t.develop.orientation.image.id())?;
            e.connect(t.develop.demosaic.output, t.develop.orientation.image)
        })
        .unwrap();
        dag.set_parameters(t.raw.node, raw.clone()).unwrap();
        let project = Project { dag, ..Default::default() };
        Project::from_json(&project.to_json().unwrap(), &registry()).unwrap();
    }
}

/// Each built-in chain develops a RAW file into finite RGB at a preview
/// scale, and the steps All adds to Neutral correction start as identities.
#[test]
fn built_in_chains_develop_a_raw() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/raw/pixls/nikon-d70s.nef");
    let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
    let global = drip::runtime::GlobalContext { scale: 8 };
    let develop = |chain: fn(&mut drip::graph::Dag) -> drip::Result<templates::Chain>| {
        let mut dag = drip::graph::Dag::new();
        let chain = chain(&mut dag).unwrap();
        dag.set_parameters(chain.raw.node, json!({ "path": fixture })).unwrap();
        let result = evaluator.evaluate::<Rgb>(&dag, &global, chain.output).unwrap();
        assert!(result.desc.extent.pixels() > 0);
        let pixels = result.data.host().unwrap().to_vec();
        assert!(pixels.iter().flatten().all(|v| v.is_finite()));
        (result.desc.extent.clone(), pixels)
    };
    develop(templates::neutral_restore);
    let corrected = develop(templates::neutral_correction);
    let all = develop(templates::all);
    assert_eq!(all.0, corrected.0);
    let worst = all
        .1
        .iter()
        .flatten()
        .zip(corrected.1.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 1e-4, "{worst}");
}

/// Neutral correction applies the lens correction the Sony fixture embeds:
/// at a preview scale the output is finite, and the vignetting gain
/// brightens the corners against the same chain with every correction off.
#[test]
fn neutral_correction_corrects_the_lens() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/raw/sony-ilce-7rm3.arw");
    let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
    let global = drip::runtime::GlobalContext { scale: 8 };
    let mut dag = drip::graph::Dag::new();
    let chain = templates::neutral_correction(&mut dag).unwrap();
    dag.set_parameters(chain.raw.node, json!({ "path": fixture })).unwrap();
    let corners = |dag: &drip::graph::Dag| {
        let result = evaluator.evaluate::<Rgb>(dag, &global, chain.output).unwrap();
        let pixels = result.data.host().unwrap();
        assert!(pixels.iter().flatten().all(|v| v.is_finite()));
        let Extent { width, height } = result.desc.extent;
        let (w, h) = (width as usize, height as usize);
        let block = |x0: usize, y0: usize| {
            let rows = (y0..y0 + 16).flat_map(|y| (x0..x0 + 16).map(move |x| y * w + x));
            rows.map(|i| pixels[i].iter().sum::<f32>()).sum::<f32>()
        };
        block(0, 0) + block(w - 16, 0) + block(0, h - 16) + block(w - 16, h - 16)
    };
    let corrected = corners(&dag);
    let lens = chain.develop.lens.as_ref().expect("Neutral correction corrects the lens");
    let off = node::lens::Settings {
        distortion: false,
        chromatic_aberration: false,
        vignetting: false,
        ..Default::default()
    };
    dag.set_parameters(lens.node, serde_json::to_value(off).unwrap()).unwrap();
    let uncorrected = corners(&dag);
    assert!(corrected > 1.1 * uncorrected, "{corrected} {uncorrected}");
}
