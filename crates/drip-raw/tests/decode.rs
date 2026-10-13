use std::path::{Path, PathBuf};

use drip_raw::{Correction, DngWarp};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw")
}

fn pixls(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/pixls").join(name)
}

fn correction(path: &Path) -> Option<Correction> {
    drip_raw::decode(path).unwrap().metadata.lens.correction
}

#[test]
fn decodes_a_bayer_raw() {
    let raw = drip_raw::decode(&fixture()).unwrap();
    assert_eq!(raw.data.len(), raw.metadata.sensor.width * raw.metadata.sensor.height);
    let mut colors: Vec<_> = raw.metadata.calibration.cfa.concat();
    colors.sort();
    assert_eq!(colors, [0, 1, 2, 3], "RGBG Bayer");
    assert!(
        raw.metadata.calibration.maximum
            > raw.metadata.calibration.black
                + raw.metadata.calibration.channel_black.iter().max().unwrap()
    );
    assert!(raw.metadata.calibration.as_shot[..3].iter().all(|&m| m > 0.0));
    assert!(raw.metadata.calibration.xyz_to_cam.iter().flatten().any(|&v| v != 0.0));
    assert_eq!((raw.metadata.make.as_str(), raw.metadata.model.as_str()), ("Sony", "ILCE-7RM3"));
    assert_eq!(raw.metadata.lens.model, "E 28-200mm F2.8-5.6 A071");
    assert_eq!(raw.metadata.lens.focal_length_35mm, 28.);
    let sensor = &raw.metadata.sensor;
    assert!(sensor.raw_width >= sensor.left_margin + sensor.width as u32);
    assert!(sensor.raw_height >= sensor.top_margin + sensor.height as u32);
    assert_eq!(sensor.pixel_aspect, 1.);
    assert_eq!(raw.metadata.orientation, 1);
    assert_eq!(raw.metadata.datetime, "2026:05:25 08:35:00", "EXIF wall-clock time");
}

#[test]
fn reports_errors() {
    let missing = drip_raw::decode(Path::new("/nonexistent/file.arw")).unwrap_err();
    assert!(!missing.is_unsupported(), "I/O failures must not be skipped");
    let not_raw = std::env::temp_dir().join(format!("drip-not-raw-{}", std::process::id()));
    std::fs::write(&not_raw, b"definitely not a raw file").unwrap();
    assert!(drip_raw::decode(&not_raw).is_err());
    // Enough bytes for format detection, rather than an early short-read error.
    std::fs::write(&not_raw, [0; 4096]).unwrap();
    let unsupported = drip_raw::decode(&not_raw).unwrap_err();
    assert!(unsupported.is_unsupported());
    std::fs::remove_file(not_raw).unwrap();
}

/// Each vendor's tags read back as exiftool 13.50 and exiv2 0.28 report them.
#[test]
fn reads_embedded_lens_corrections() {
    assert_eq!(
        correction(&fixture()),
        Some(Correction::Sony {
            distortion: vec![
                7, 0, -10, -22, -38, -56, -76, -99, -123, -146, -168, -187, -203, -215, -221, -218
            ],
            ca_red: vec![
                1152, 1152, 1152, 1152, 1152, 1152, 1152, 1024, 1024, 1024, 1024, 1024, 1024, 1024,
                896, 896
            ],
            ca_blue: vec![
                128, 128, 128, 0, 0, 128, 128, 128, 256, 384, 384, 512, 640, 768, 768, 768
            ],
            vignetting: vec![
                0, 32, 128, 320, 544, 832, 1184, 1600, 2080, 2592, 3200, 3968, 4864, 5952, 7200,
                8384
            ],
        })
    );

    let ratios = |n: &[i32], d: i32| n.iter().map(|&n| n as f32 / d as f32).collect::<Vec<_>>();
    assert_eq!(
        correction(&pixls("fujifilm-gfx50s-ii.raf")),
        Some(Correction::Fuji {
            knots: ratios(&[967, 1368, 1675, 1934, 2162, 2369, 2558, 2735, 2901], 2735),
            distortion: ratios(&[1789, 3578, 4842, 5473, 5473, 4210, 3368, 947, 947], 65536),
            ca_red: ratios(&[-4, -4, -2, -2, 0, 0, -2, -4, -6], 65536),
            ca_blue: ratios(&[0, 0, -2, -2, 0, 0, 0, 2, 4], 65536),
            vignetting: ratios(
                &[178937, 163712, 152409, 142760, 133875, 124979, 116153, 108100, 108100],
                2048
            ),
            crop_mode: Some(0),
        })
    );

    assert_eq!(
        correction(&pixls("google-pixel-4-xl.dng")),
        Some(Correction::Dng {
            warp: Some(DngWarp {
                planes: vec![[
                    0.9805405735969543,
                    0.052180830389261246,
                    -0.052247390151023865,
                    0.01846814714372158,
                    0.,
                    0.
                ]],
                centre: [0.46570233319256754, 0.5025973165301685],
            }),
            vignette: None,
        })
    );

    // exiftool prints these floats with 15 digits, which round back exactly.
    let floats = |v: [f64; 6]| v.map(|v| v as f32);
    let [d0, d1, d2, d3, ..] =
        floats([-0.115105330944061, -0.00867846980690956, 0.00447992002591491, 1., 0., 0.]);
    assert_eq!(
        correction(&pixls("om-system-tg-7.orf")),
        Some(Correction::Olympus {
            distortion: Some([d0, d1, d2, d3]),
            ca: Some(floats([
                0.000397227006033063,
                0.000129881504108198,
                0.,
                0.000526041607372463,
                0.000298548198770732,
                0.
            ])),
        })
    );

    // exiftool prints the Q15 words divided by 32768.
    let Some(Correction::Panasonic { a, b, c, scale }) =
        correction(&pixls("panasonic-dmc-tz70.rw2"))
    else {
        panic!("Panasonic DistortionInfo")
    };
    let q15 = |w: i16| w as f64 / 32768.;
    assert_eq!(
        [a, b, c, scale].map(q15),
        [0.1810302734375, 0.335723876953125, -0.35101318359375, 0.]
    );
}

#[test]
fn files_without_lens_corrections_read_none() {
    // Nikon Z bodies carry coefficients darktable does not read.
    for name in [
        "sony-ilce-7s.arw",
        "olympus-e-3.orf",
        "panasonic-dmc-lx7.rw2",
        "leica-m8.dng",
        "ricoh-gr.dng",
        "nikon-z6ii.nef",
    ] {
        assert_eq!(correction(&pixls(name)), None, "{name}");
    }
}
