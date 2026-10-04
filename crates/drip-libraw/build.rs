//! Links the system's thread-safe LibRaw (`libraw_r`) and compiles the C shim
//! against its headers. A static build of LibRaw with only the needed features
//! is planned (TODO.md). Assumes host = target, as native builds do.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=shim/shim.c");
    let include_paths = find_libraw();
    cc::Build::new()
        .file("shim/shim.c")
        .includes(include_paths.iter().map(|p| p.join("libraw")))
        .includes(include_paths)
        .compile("drip_libraw_shim");
}

#[cfg(windows)]
fn find_libraw() -> Vec<PathBuf> {
    vcpkg::find_package("libraw").expect("LibRaw not found through vcpkg").include_paths
}

#[cfg(not(windows))]
fn find_libraw() -> Vec<PathBuf> {
    pkg_config::probe_library("libraw_r")
        .expect("libraw_r not found through pkg-config")
        .include_paths
}
