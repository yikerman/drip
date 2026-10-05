//! Links the system's thread-safe LibRaw (`libraw_r`) and compiles the C shim
//! against its headers. A static build of LibRaw with only the needed features
//! is planned (TODO.md). Assumes host = target, as native builds do.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=shim/shim.c");
    let include_paths = find_libraw();
    cc::Build::new()
        .file("shim/shim.c")
        .std("c11")
        .includes(include_paths.iter().map(|p| p.join("libraw")))
        .includes(include_paths)
        .compile("drip_libraw_shim");
}

#[cfg(windows)]
fn find_libraw() -> Vec<PathBuf> {
    vcpkg::find_package("libraw").expect("LibRaw not found through vcpkg").include_paths
}

/// Links what pkg-config lists except `stdc++`: LibRaw's upstream `.pc` file
/// names libstdc++, which macOS doesn't have (it uses libc++), while the shared
/// library already depends on whichever C++ runtime it was built with.
#[cfg(not(windows))]
fn find_libraw() -> Vec<PathBuf> {
    let lib = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("libraw_r")
        .expect("libraw_r not found through pkg-config");
    for path in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for name in lib.libs.iter().filter(|name| *name != "stdc++") {
        println!("cargo:rustc-link-lib={name}");
    }
    lib.include_paths
}
