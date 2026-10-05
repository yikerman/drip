//! The pipeline on a real raw.

use drip::image::{LinearThreeChannelMatrix, Mosaic};
use drip::ports::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use drip::eval::{Evaluator, run_action};
use drip::graph::{NodeId, Port};
use drip::nodes;
use drip::project::Project;
use lcms2::Profile;
use serde_json::json;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw")
}

/// raw.read → white balance → binning → camera to Rec.2020 → kinds….
fn pipeline(raw: &Path, tail: &[&'static drip::node::NodeKind]) -> (Project, Vec<NodeId>) {
    let mut p = Project::default();
    let read = p.graph.add_node(&nodes::READ);
    p.graph.set_param(read, "path", json!(raw)).unwrap();
    let mut ids = vec![read];
    for kind in
        [&nodes::WHITE_BALANCE, &nodes::BIN_2X2, &nodes::CAMERA_TO_REC2020].iter().chain(tail)
    {
        let (last, id) = (*ids.last().unwrap(), p.graph.add_node(kind));
        let output = p.graph.node(last).unwrap().kind.outputs().next().unwrap().name;
        p.graph
            .connect(Port(last, output.into()), Port(id, kind.inputs().next().unwrap().name.into()))
            .unwrap();
        ids.push(id);
    }
    (p, ids)
}

/// Compares Drip's scene-referred Rec.2020 with LibRaw's own half-size
/// processing, which shares nothing with Drip past the decoded raw.
#[test]
fn pipeline_matches_libraw() {
    let path = fixture();
    let (p, ids) = pipeline(&path, &[]);
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, 0, &[ids[3]]);
    let ours = ev.result(ids[3]).unwrap().as_ref().unwrap().outputs[0]
        .borrow::<Read<dyn LinearThreeChannelMatrix>>()
        .unwrap()
        .rgb()
        .clone();
    let (width, _, reference) = drip_libraw::reference(&path).unwrap();

    // LibRaw scales by 65535 and normalizes white balance to its smallest
    // multiplier rather than green, so compare up to one global factor,
    // estimated on unclipped mid-tones and checked against its prediction.
    let usable = |r: &[u16; 3]| r.iter().all(|&v| (1000..60000).contains(&v));
    let pairs: Vec<_> = (0..ours.height)
        .flat_map(|y| (0..ours.width).map(move |x| (y, x)))
        .map(|(y, x)| (ours.pixels[y * ours.width + x], reference[y * width + x]))
        .filter(|(_, r)| usable(r))
        .collect();
    assert!(pairs.len() > 10_000, "too few usable pixels");
    let mut ratios: Vec<_> = pairs.iter().map(|(o, r)| f64::from(r[1]) / f64::from(o[1])).collect();
    ratios.sort_by(f64::total_cmp);
    let scale = ratios[ratios.len() / 2];
    let m = drip_libraw::decode(&path).unwrap().as_shot;
    let expected = 65535.0 * f64::from(m[1] / m[..3].iter().copied().fold(f32::INFINITY, f32::min));
    assert!((scale / expected - 1.0).abs() < 1e-3, "scale {scale}, expected {expected}");

    let mut errors: Vec<_> = pairs
        .iter()
        .flat_map(|(o, r)| {
            (0..3).map(move |c| (f64::from(o[c]) * scale / f64::from(r[c]) - 1.0).abs())
        })
        .collect();
    errors.sort_by(f64::total_cmp);
    let (median, p99) = (errors[errors.len() / 2], errors[errors.len() * 99 / 100]);
    assert!(median < 2e-3 && p99 < 2e-2, "relative error median {median}, p99 {p99}");
}

#[test]
fn exports_a_tiff_from_a_raw() {
    let dir = std::env::temp_dir().join(format!("drip-raw-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (profile, out) = (dir.join("srgb.icc"), dir.join("out.tif"));
    std::fs::write(&profile, Profile::new_srgb().icc().unwrap()).unwrap();

    let mut p = drip::templates::raw_to_tiff();
    let read = p.graph.find("raw").unwrap();
    let export = p.graph.find("export").unwrap();
    p.graph.set_param(read, "path", json!(fixture())).unwrap();
    p.graph.set_param(export, "path", json!(out)).unwrap();
    p.graph.set_param(export, "profile", json!("file")).unwrap();
    p.graph.set_param(export, "profile_file", json!(profile)).unwrap();
    run_action(&p.graph, export, "export").unwrap();

    let raw = drip_libraw::decode(&fixture()).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(
        decoder.dimensions().unwrap(),
        (raw.width as u32 / 2 * 2, raw.height as u32 / 2 * 2)
    );
    let icc = decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap();
    assert_eq!(icc, std::fs::read(&profile).unwrap());
    let tiff::decoder::DecodingResult::U16(data) = decoder.read_image().unwrap() else {
        panic!("not 16 bit")
    };
    let mean = data.iter().map(|&v| f64::from(v)).sum::<f64>() / data.len() as f64 / 65535.0;
    assert!((0.02..0.98).contains(&mean), "mean {mean}: blank or saturated");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn built_in_template_takes_the_raw_and_output_paths() {
    let template = drip::templates::raw_to_tiff();
    let (raw, export) =
        (template.graph.find("raw").unwrap(), template.graph.find("export").unwrap());
    assert_eq!(template.graph.inputs().collect::<Vec<_>>(), [(raw, "path"), (export, "path")]);
    assert_eq!(Project::from_json(&template.to_json(), &nodes::registry()).unwrap(), template);

    let mut p = template;
    let path = std::env::temp_dir().join(format!("drip-raw-cache-{}.arw", std::process::id()));
    std::fs::copy(fixture(), &path).unwrap();
    p.graph.set_param(raw, "path", json!(path)).unwrap();
    let preview = p.graph.find("preview").unwrap();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, 3, &[preview]);
    // Later levels and export forks must work entirely from decoded data.
    std::fs::remove_file(&path).unwrap();
    let view = ev.result(preview).unwrap().as_ref().unwrap().view.clone();
    let Some(drip::view::View::Image(image)) = view else { panic!("no preview") };
    assert_eq!(image.rgb().scale, 8, "RCD preserves the requested scale");

    let cached = ev.result(raw).unwrap().as_ref().unwrap().outputs[0].downcast::<Mosaic>().unwrap();
    let previous = Arc::downgrade(&cached);
    let expected = cached.data.clone();
    drop(cached);
    ev.evaluate(&p.graph, 2, &[raw]);
    assert!(previous.upgrade().is_none(), "old normalized levels are not retained");
    ev.evaluate(&p.graph, 3, &[raw]);
    assert_eq!(
        expected,
        ev.result(raw).unwrap().as_ref().unwrap().outputs[0].downcast_ref::<Mosaic>().unwrap().data
    );

    let other = p.graph.add_node(&nodes::READ);
    p.graph.set_param(other, "path", json!(path)).unwrap();
    let mut fork = ev.fork();
    fork.evaluate(&p.graph, 3, &[other]);
    assert_eq!(
        expected,
        fork.result(other).unwrap().as_ref().unwrap().outputs[0]
            .downcast_ref::<Mosaic>()
            .unwrap()
            .data
    );
    fork.evaluate(&p.graph, 0, &[other]);
    ev.evaluate(&p.graph, 0, &[raw]);
    assert_eq!(
        fork.result(other).unwrap().as_ref().unwrap().outputs[0]
            .downcast_ref::<Mosaic>()
            .unwrap()
            .data,
        ev.result(raw).unwrap().as_ref().unwrap().outputs[0].downcast_ref::<Mosaic>().unwrap().data,
    );
    let demosaic = p.graph.find("demosaic").unwrap();
    ev.evaluate(&p.graph, 31, &[demosaic]);
    let smallest = ev.result(demosaic).unwrap().as_ref().unwrap().outputs[0]
        .borrow::<Read<dyn LinearThreeChannelMatrix>>()
        .unwrap()
        .rgb();
    assert!(smallest.pixels.is_empty());
    assert_eq!(smallest.scale, 1 << 31);
    let sensor =
        ev.result(raw).unwrap().as_ref().unwrap().outputs[0].downcast_ref::<Mosaic>().unwrap();
    assert_eq!(sensor.scale, 1, "sensor processing precedes preview reduction");
}
