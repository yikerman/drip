//! Builds the pinned RAW stack without consulting system codec libraries.

#[path = "build/codecs.rs"]
mod codecs;
#[path = "build/libraw.rs"]
mod libraw;

use std::{env, path::PathBuf};

fn main() {
    let vendor = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../vendor");
    for (name, marker) in
        [("libraw", "Makefile.am"), ("libjpeg-turbo", "CMakeLists.txt"), ("zlib", "CMakeLists.txt")]
    {
        let source = vendor.join(name);
        assert!(
            source.join(marker).is_file(),
            "missing vendor/{name}, run `git submodule update --init --recursive`"
        );
        println!("cargo:rerun-if-changed={}", source.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build");
    println!("cargo:rerun-if-changed=shim/shim.c");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let jpeg = codecs::jpeg(&vendor);
    let zlib = codecs::zlib(&vendor, &target_os);

    // Emit libraries in dependency order: shim -> LibRaw -> codecs.
    let mut shim = cc::Build::new();
    shim.file("shim/shim.c")
        .std("c11")
        .define("LIBRAW_NODLL", None)
        .include(vendor.join("libraw/libraw"));
    if env::var_os("CARGO_FEATURE_REFERENCE").is_some() {
        shim.define("DRIP_REFERENCE", None);
    }
    shim.compile("drip_raw_shim");
    libraw::build(&vendor.join("libraw"), &jpeg, &zlib, &target_os);
    jpeg.link();
    zlib.link();
}
