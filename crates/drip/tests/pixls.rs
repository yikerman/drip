//! Cross-camera contracts, not a photographic ground truth. Retain TIFFs for
//! visual review with DRIP_RAW_REVIEW_DIR=/path cargo test -p drip --test pixls.

use std::path::{Path, PathBuf};

use drip::eval::Evaluator;
use serde_json::json;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

#[test]
fn camera_corpus_resolution_and_export() {
    // Camera names are LibRaw's normalized names, including regional aliases.
    let fixtures = [
        ("nikon-d70s.nef", "Nikon", "D70s"),
        ("nikon-z6ii.nef", "Nikon", "Z 6_2"),
        ("sony-ilce-7s.arw", "Sony", "ILCE-7S"),
        ("fujifilm-finepix-s5500.raf", "Fujifilm", "S5100"),
        ("olympus-e-3.orf", "Olympus", "E-3"),
        ("panasonic-dmc-lx7.rw2", "Panasonic", "DMC-LX7"),
        ("pentax-k10d.pef", "Pentax", "K10D"),
        ("leica-m8.dng", "Leica", "M8"),
        ("kodak-dcs520c.tif", "Kodak", "DCS520C"),
        ("minolta-dynax-7d.mrw", "Minolta", "DG-7D"),
        ("phase-one-iq140.tif", "Phase One", "IQ140"),
        ("ricoh-gr.dng", "Ricoh", "GR"),
        ("canon-eos-350d-raw-3-2.cr2", "Canon", "EOS 350D"),
        ("samsung-nx10-12bit-3-2.srw", "Samsung", "NX10"),
        ("../sony-ilce-7rm3.arw", "Sony", "ILCE-7RM3"),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/pixls");
    let review = std::env::var_os("DRIP_RAW_REVIEW_DIR").map(PathBuf::from);
    let output = review
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("drip-pixls-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    // Both paths load the same serialized profile: ICC serialization itself
    // quantizes matrices, which would otherwise contaminate pixel comparisons.
    let profile_path = output.join("srgb.icc");
    let icc = lcms2::Profile::new_srgb().icc().unwrap();
    std::fs::write(&profile_path, &icc).unwrap();
    let mut passed = 0;
    let mut skipped = 0;
    for (file, make, model) in fixtures {
        eprintln!("TEST {make} {model} ({file})");
        let path = root.join(file);
        let raw = match drip_libraw::decode(&path) {
            Ok(raw) => raw,
            Err(error) if error.is_unsupported() => {
                eprintln!("SKIP {}: LibRaw: {error}", file);
                skipped += 1;
                continue;
            }
            Err(error) => panic!("{}: {error}", file),
        };
        eprintln!(
            "  {} {}: {}x{}, CFA {:?}",
            raw.metadata.make, raw.metadata.model, raw.width, raw.height, raw.cfa
        );
        assert!(raw.metadata.make.eq_ignore_ascii_case(make));
        assert!(raw.metadata.model.eq_ignore_ascii_case(model));
        assert_eq!(raw.data.len(), raw.width * raw.height);
        assert!(raw.width > 0 && raw.height > 0);
        let mut colors = raw.cfa.concat();
        // LibRaw can use either green index for the second green.
        colors.iter_mut().for_each(|c| {
            if *c == 3 {
                *c = 1;
            }
        });
        colors.sort_unstable();
        assert_eq!(colors, [0, 1, 1, 2]);
        assert!(raw.data.iter().min() < raw.data.iter().max(), "constant sensor data");

        let mut project = drip::templates::raw_to_tiff();
        let read = project.graph.find("RAW").unwrap();
        let preview = project.graph.find("Preview").unwrap();
        let export = project.graph.find("Export").unwrap();
        let out = output.join(format!("{}.tiff", path.file_name().unwrap().to_str().unwrap()));
        project.graph.set_param(read, "path", json!(path)).unwrap();
        project.graph.set_param(export, "path", json!(out)).unwrap();
        project.graph.set_param(export, "compression", json!("none")).unwrap();
        project.graph.set_param(export, "profile", json!("file")).unwrap();
        project.graph.set_param(export, "profile_file", json!(profile_path)).unwrap();
        let mut evaluator = Evaluator::default();
        let mut full = None;
        for level in [3, 0] {
            evaluator.evaluate(&project.graph, level, &[preview]);
            let failures: Vec<_> = evaluator.failures(&project.graph, &[preview]).collect();
            assert!(failures.is_empty(), "{}: {failures:?}", file);
            let rgb = evaluator
                .with_inputs(
                    &project.graph,
                    preview,
                    level,
                    &drip::nodes::PREVIEW,
                    |_, (image,), _| Ok(image.rgb().clone()),
                )
                .unwrap();
            let scale = 1 << level;
            assert_eq!(rgb.scale, scale);
            let scale = scale as usize;
            assert_eq!(
                (rgb.width, rgb.height),
                (raw.width / (2 * scale) * 2, raw.height / (2 * scale) * 2)
            );
            assert_eq!(rgb.pixels.len(), rgb.width * rgb.height);
            assert!(
                rgb.pixels.iter().flatten().all(|v| v.is_finite()),
                "nonfinite processing output"
            );
            if level == 0 {
                full = Some(rgb.clone());
            }
        }
        // Export reevaluates at full resolution. A separately read TIFF must
        // agree with the computational output after its declared ICC conversion.
        evaluator.run_action(&project.graph, export, "export").unwrap();
        let full = full.unwrap();
        let mut decoder = Decoder::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (full.width as u32, full.height as u32));
        assert_eq!(decoder.get_tag_ascii_string(Tag::Make).unwrap(), raw.metadata.make);
        assert_eq!(decoder.get_tag_ascii_string(Tag::Model).unwrap(), raw.metadata.model);
        let embedded = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
        assert_eq!(embedded, icc);
        let profile = lcms2::Profile::new_icc(&embedded).unwrap();
        let transform = lcms2::Transform::<[f32; 3], [u16; 3]>::new_flags(
            &drip::profile::rec2020_linear(),
            lcms2::PixelFormat::RGB_FLT,
            &profile,
            lcms2::PixelFormat::RGB_16,
            lcms2::Intent::RelativeColorimetric,
            lcms2::Flags::BLACKPOINT_COMPENSATION | lcms2::Flags::NO_CACHE,
        )
        .unwrap();
        let DecodingResult::U16(data) = decoder.read_image().unwrap() else {
            panic!("not a 16-bit TIFF")
        };
        assert_eq!(data.len(), full.pixels.len() * 3);
        let mut expected = vec![[0u16; 3]; full.pixels.len()];
        transform.transform_pixels(&full.pixels, &mut expected);
        assert!(
            data.iter().zip(expected.as_flattened()).all(|(a, b)| a.abs_diff(*b) <= 1),
            "TIFF differs from full-resolution processing output after ICC conversion"
        );
        eprintln!("PASS {}", file);
        passed += 1;
        if review.is_none() {
            std::fs::remove_file(out).unwrap();
        }
    }
    eprintln!("camera corpus: {passed} rendered, {skipped} skipped (LibRaw unsupported)");
    assert!(passed > 0, "no supported fixtures were tested");
    if review.is_none() {
        std::fs::remove_file(profile_path).unwrap();
        std::fs::remove_dir(output).unwrap();
    }
}
