use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw")
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
    assert_eq!(
        (raw.metadata.make.as_slice(), raw.metadata.model.as_slice()),
        (b"Sony".as_slice(), b"ILCE-7RM3".as_slice())
    );
    assert_eq!(raw.metadata.lens.model, b"E 28-200mm F2.8-5.6 A071");
    assert_eq!(raw.metadata.lens.focal_length_35mm, 28.);
    let sensor = &raw.metadata.sensor;
    assert!(sensor.raw_width >= sensor.left_margin + sensor.width as u32);
    assert!(sensor.raw_height >= sensor.top_margin + sensor.height as u32);
    assert_eq!(sensor.pixel_aspect, 1.);
    assert_eq!(raw.metadata.orientation, 1);
    for text in raw.metadata.text_fields() {
        assert!(std::str::from_utf8(text).is_ok());
    }
    assert_eq!(raw.metadata.datetime, b"2026:05:25 08:35:00", "EXIF wall-clock time");
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
