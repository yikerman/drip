//! Select compute support for the artifact's OS, including cross builds.
use std::{env, process::Command};

use crate::Result;

pub(super) fn resolve(target: Option<&str>) -> Result<(String, &'static str)> {
    let target = match target {
        Some(target) => target.to_owned(),
        None => rustc(&["-vV"])?
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .ok_or("rustc did not report its host target")?
            .to_owned(),
    };
    let cfg = rustc(&["--print", "cfg", "--target", &target])?;
    let os = cfg
        .lines()
        .find_map(|line| line.strip_prefix("target_os=\"")?.strip_suffix('"'))
        .ok_or("rustc did not report target_os")?;
    let feature = match os {
        "linux" => "drip/vulkan",
        "macos" => "drip/metal-native",
        "windows" => "drip/wgpu",
        _ => return Err(format!("unsupported distribution platform: {os} ({target})").into()),
    };
    Ok((target, feature))
}

fn rustc(args: &[&str]) -> Result<String> {
    let output =
        Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())).args(args).output()?;
    if !output.status.success() {
        return Err(format!("rustc failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_targets_use_the_destination_platform() {
        assert!(resolve(None).is_ok());
        for (target, feature) in [
            ("x86_64-unknown-linux-gnu", "drip/vulkan"),
            ("aarch64-unknown-linux-gnu", "drip/vulkan"),
            ("x86_64-apple-darwin", "drip/metal-native"),
            ("aarch64-apple-darwin", "drip/metal-native"),
            ("x86_64-pc-windows-msvc", "drip/wgpu"),
            ("aarch64-pc-windows-msvc", "drip/wgpu"),
        ] {
            assert_eq!(resolve(Some(target)).unwrap(), (target.into(), feature));
        }
        assert!(resolve(Some("wasm32-unknown-unknown")).is_err());
        assert!(resolve(Some("not-a-rust-target")).is_err());
    }
}
