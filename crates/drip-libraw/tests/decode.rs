//! Runs against real raws when `DRIP_TEST_RAWS` names a directory of them;
//! otherwise there is nothing to check and the tests pass vacuously.

use std::path::{Path, PathBuf};

fn raws(extension: &str) -> Vec<PathBuf> {
    let Some(dir) = std::env::var_os("DRIP_TEST_RAWS") else { return vec![] };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)))
        .collect();
    files.sort();
    files.truncate(3);
    files
}

#[test]
fn decodes_bayer_raws() {
    for path in raws("arw") {
        let raw = drip_libraw::decode(&path).unwrap();
        assert_eq!(raw.data.len(), raw.width * raw.height);
        let mut colors: Vec<_> = raw.cfa.concat();
        colors.sort();
        assert_eq!(colors, [0, 1, 2, 3], "RGBG Bayer");
        assert!(raw.maximum > raw.black + raw.channel_black.iter().max().unwrap());
        assert!(raw.as_shot[..3].iter().all(|&m| m > 0.0));
        eprintln!(
            "{}x{} cfa {:?} black {} {:?} pattern {}x{} max {} wb {:?}",
            raw.width,
            raw.height,
            raw.cfa,
            raw.black,
            raw.channel_black,
            raw.pattern.height,
            raw.pattern.width,
            raw.maximum,
            raw.as_shot
        );
    }
}

#[test]
fn reports_errors() {
    assert!(drip_libraw::decode(Path::new("/nonexistent/file.arw")).is_err());
    let not_raw = std::env::temp_dir().join(format!("drip-not-raw-{}", std::process::id()));
    std::fs::write(&not_raw, b"definitely not a raw file").unwrap();
    assert!(drip_libraw::decode(&not_raw).is_err());
    std::fs::remove_file(not_raw).unwrap();
}
