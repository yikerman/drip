use std::{fs, path::Path};

use crate::codecs::Codec;

pub fn build(source: &Path, jpeg: &Codec, zlib: &Codec, target_os: &str) {
    // Use the pinned upstream source list shared by raw/raw_r (THIRD_PARTY [18]).
    // Only parse that declaration; upstream owns which translation units belong.
    let makefile = fs::read_to_string(source.join("Makefile.am")).unwrap().replace("\r\n", "\n");
    let sources = makefile
        .split_once("lib_libraw_a_SOURCES = ")
        .unwrap()
        .1
        .split("\n\n")
        .next()
        .unwrap()
        .replace("\\\n", " ");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .warnings(false)
        .include(source)
        .include(jpeg.prefix.join("include"))
        .include(zlib.prefix.join("include"))
        .define("USE_JPEG", None)
        .define("USE_ZLIB", None)
        .define("LIBRAW_NODLL", None)
        .files(sources.split_whitespace().map(|file| source.join(file)));
    // Match upstream MSVC settings: decoder exceptions must unwind local allocations.
    if build.get_compiler().is_like_msvc() {
        build.flag("/EHsc");
    }
    // No LIBRAW_NOTHREADS or OpenMP: separate decoder instances are reentrant.
    if target_os != "windows" {
        build.flag("-pthread");
    }
    build.compile("raw_r");
    if target_os == "windows" {
        println!("cargo:rustc-link-lib=ws2_32");
    }
}
