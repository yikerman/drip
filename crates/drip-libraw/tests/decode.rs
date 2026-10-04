use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw")
}

#[test]
fn decodes_a_bayer_raw() {
    let raw = drip_libraw::decode(&fixture()).unwrap();
    assert_eq!(raw.data.len(), raw.width * raw.height);
    let mut colors: Vec<_> = raw.cfa.concat();
    colors.sort();
    assert_eq!(colors, [0, 1, 2, 3], "RGBG Bayer");
    assert!(raw.maximum > raw.black + raw.channel_black.iter().max().unwrap());
    assert!(raw.as_shot[..3].iter().all(|&m| m > 0.0));
    assert!(raw.xyz_to_cam.iter().flatten().any(|&v| v != 0.0));
    assert_eq!((raw.metadata.make.as_str(), raw.metadata.model.as_str()), ("Sony", "ILCE-7RM3"));
}

#[test]
fn reports_errors() {
    assert!(drip_libraw::decode(Path::new("/nonexistent/file.arw")).is_err());
    let not_raw = std::env::temp_dir().join(format!("drip-not-raw-{}", std::process::id()));
    std::fs::write(&not_raw, b"definitely not a raw file").unwrap();
    assert!(drip_libraw::decode(&not_raw).is_err());
    std::fs::remove_file(not_raw).unwrap();
}
