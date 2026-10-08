use std::{
    env,
    path::{Path, PathBuf},
};

pub struct Codec {
    pub prefix: PathBuf,
    library: &'static str,
}

impl Codec {
    pub fn link(&self) {
        println!("cargo:rustc-link-search=native={}", self.prefix.join("lib").display());
        println!("cargo:rustc-link-lib=static={}", self.library);
    }
}

fn configure(source: &Path) -> cmake::Config {
    let mut config = cmake::Config::new(source);
    config
        .out_dir(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(source.file_name().unwrap()))
        .profile("Release")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
    config
}

pub fn jpeg(vendor: &Path) -> Codec {
    let static_crt = env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .split(',')
        .any(|feature| feature == "crt-static");
    let prefix = configure(&vendor.join("libjpeg-turbo"))
        // Keep the embedded build identifier independent of the wall clock.
        .define("BUILD", "drip")
        .define("ENABLE_SHARED", "OFF")
        .define("ENABLE_STATIC", "ON")
        .define("WITH_TURBOJPEG", "OFF")
        .define("WITH_TOOLS", "OFF")
        .define("WITH_TESTS", "OFF")
        .define("WITH_SIMD", "ON")
        .define("REQUIRE_SIMD", "ON")
        .define("WITH_CRT_DLL", if static_crt { "OFF" } else { "ON" })
        .build();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc";
    Codec { prefix, library: if msvc { "jpeg-static" } else { "jpeg" } }
}

pub fn zlib(vendor: &Path, target_os: &str) -> Codec {
    let prefix = configure(&vendor.join("zlib"))
        .define("ZLIB_BUILD_SHARED", "OFF")
        .define("ZLIB_BUILD_STATIC", "ON")
        .define("ZLIB_BUILD_TESTING", "OFF")
        .build();
    Codec { prefix, library: if target_os == "windows" { "zs" } else { "z" } }
}
