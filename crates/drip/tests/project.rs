use drip::{
    eval::Evaluator, node, node::data::*, ports::*, project::Project, runtime::RuntimeContext,
};
use serde_json::json;
#[test]
fn project_round_trip_uses_named_ports_and_revalidates_parameters() {
    let mut p = Project::default();
    let first = node::sigmoid::apply::add(&mut p.dag, Default::default()).unwrap();
    let apply = node::sigmoid::apply::add(&mut p.dag, Default::default()).unwrap();
    p.dag.connect(first.output, apply.image).unwrap();
    p.target = Some(first.output.id());
    p.ui = json!({"retained":true});
    p.node_ui.insert(apply.node, json!({"pos":[12,34]}));
    let serialized = p.to_json().unwrap();
    let restored = Project::from_json(&serialized).unwrap();
    assert_eq!(restored.dag.edges().count(), 1);
    assert_eq!(restored.ui, p.ui);
    assert_eq!(restored.to_json().unwrap(), serialized);
    let output = restored.dag.typed_output::<ColorRgb>(restored.target.unwrap()).unwrap();
    // An incomplete image pipeline remains editable after loading.
    assert!(restored.dag.description(output).unwrap().is_none());
    let mut data: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    data["nodes"][0]["parameters"]["contrast"] = json!(-1.);
    assert!(Project::from_json(&data.to_string()).is_err());
    data = serde_json::from_str(&serialized).unwrap();
    data["edges"][0][0]["port"] = json!("missing");
    assert!(Project::from_json(&data.to_string()).is_err());
    assert!(Project::from_json("{\"version\":1}").is_err());
}
#[test]
fn tiff_profile_extent_and_pixels_survive_encoding() {
    let desc = ImageDesc {
        extent: Extent { width: 2, height: 1 },
        interpretation: node::raw::working_color(),
    };
    let profile =
        drip::node::profile::Output::load(&Default::default(), &Default::default()).unwrap();
    let bytes =
        drip::node::export::tiff(&desc, &[[0.; 3], [1.; 3]], &profile, false, None).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(decoder.dimensions().unwrap(), (2, 1));
    assert!(!decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap().is_empty());
    let tiff::decoder::DecodingResult::U16(pixels) = decoder.read_image().unwrap() else {
        panic!("expected u16")
    };
    assert_eq!(&pixels[..3], &[0, 0, 0]);
    assert!(pixels[3..].iter().all(|&v| v > 65530));
    let mut bad = desc.clone();
    bad.interpretation.encoding = Encoding::Srgb;
    assert!(drip::node::export::tiff(&bad, &[[0.; 3], [1.; 3]], &profile, false, None).is_err());
}

#[test]
fn headless_target_load_ignores_frontend_nodes_but_validates_demanded_graph() {
    let mut p = Project::default();
    let first = node::sigmoid::apply::add(&mut p.dag, Default::default()).unwrap();
    p.target = Some(first.output.id());
    let mut document: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
    document["nodes"].as_array_mut().unwrap().push(json!({
        "kind": "unregistered.frontend", "parameters": null, "ui": null
    }));
    document["edges"] = json!([[{"node":0,"port":"output"}, {"node":1,"port":"input"}]]);
    assert!(Project::from_json(&document.to_string()).is_err());
    let loaded = Project::target_from_json(&document.to_string()).unwrap();
    assert_eq!(loaded.dag.nodes().count(), 1);
    assert!(loaded.dag.edges().next().is_none());
    let mut invalid = document.clone();
    invalid["target"]["node"] = json!(1);
    assert!(Project::target_from_json(&invalid.to_string()).is_err());
    invalid = document.clone();
    invalid["nodes"][0]["parameters"]["contrast"] = json!(-1.);
    assert!(Project::target_from_json(&invalid.to_string()).is_err());
    invalid = document.clone();
    invalid["target"]["node"] = json!(99);
    assert!(Project::target_from_json(&invalid.to_string()).is_err());
    invalid = document;
    invalid["edges"] = json!([[{"node":0,"port":"output"}, {"node":0,"port":"invalid"}]]);
    assert!(Project::target_from_json(&invalid.to_string()).is_err());
}

#[test]
fn missing_raw_preserves_editable_parameters_and_reports_load_error() {
    let path = std::path::Path::new("/tmp/drip-deliberately-missing-raw-source.nef");
    assert!(!path.exists());
    let project = drip::node::templates::raw_to_rgb(path).unwrap();
    let text = project.to_json().unwrap();
    let mut restored = Project::from_json(&text).unwrap();
    assert_eq!(restored.to_json().unwrap(), text);
    let target = restored.dag.typed_output::<ColorRgb>(restored.target.unwrap()).unwrap();
    let error = Evaluator::new(RuntimeContext::host())
        .evaluate::<Cpu<ColorRgb>>(&restored.dag, &Default::default(), target)
        .err()
        .unwrap();
    assert!(error.to_string().contains("drip-deliberately-missing-raw-source.nef"));
    let raw = restored.dag.nodes().find(|(_, n)| n.metadata().id == "input.bayer-raw").unwrap().0;
    let values = Evaluator::new(RuntimeContext::host()).evaluate_inputs(
        &restored.dag,
        &Default::default(),
        &[target.node()],
    );
    assert!(matches!(&values[&target.node()], Err(drip::Error::Node { node, source, .. })
        if *node == raw && source.to_string().contains("drip-deliberately-missing-raw-source.nef")));
    restored.dag.set_parameters(raw, json!({"path":null})).unwrap();
    assert!(restored.dag.node(raw).unwrap().parameters().unwrap()["path"].is_null());
}

#[test]
fn bound_raw_snapshot_survives_file_removal_until_explicit_rebind() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/raw/pixls/nikon-d70s.nef");
    let temporary =
        std::env::temp_dir().join(format!("drip-raw-snapshot-{}.nef", std::process::id()));
    std::fs::copy(&fixture, &temporary).unwrap();
    let mut dag = drip::graph::Dag::new();
    let source = dag
        .add(node::raw::RawSource::bind(node::raw::Settings { path: Some(temporary.clone()) }))
        .unwrap();
    std::fs::remove_file(&temporary).unwrap();
    let output = dag.typed_output::<Bayer>(dag.output_port(source, "mosaic").unwrap()).unwrap();
    let evaluator = Evaluator::new(RuntimeContext::host());
    assert!(evaluator.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), output).is_ok());
    dag.set_parameters(source, json!({"path":temporary})).unwrap();
    assert!(evaluator.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), output).is_err());
    dag.set_parameters(source, json!({"path":fixture})).unwrap();
    assert!(evaluator.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), output).is_ok());
}
