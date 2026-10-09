//! Build features include runtimes; DRIP_BACKEND selects one without fallback.
use super::RuntimeContext;
use crate::{Error, Result};
#[cfg(any(
    feature = "wgpu",
    feature = "cpu",
    feature = "cuda",
    feature = "hip",
    all(feature = "metal-native", target_os = "macos")
))]
use cubecl::__private::Runtime;

impl RuntimeContext {
    /// WGSL through wgpu. Windows uses D3D12; other platforms use wgpu's
    /// automatic API selection. Enabling native compilers does not change this.
    #[cfg(feature = "wgpu")]
    pub fn wgpu() -> Self {
        #[cfg(target_os = "windows")]
        let api = cubecl::wgpu::WgpuBackend::Dx12;
        #[cfg(not(target_os = "windows"))]
        let api = cubecl::wgpu::WgpuBackend::Auto;
        wgpu_client::<cubecl::wgpu::WgslCompiler>(api)
    }

    /// Direct SPIR-V compilation on Vulkan; never substitutes WGSL.
    #[cfg(feature = "vulkan")]
    pub fn vulkan() -> Self {
        wgpu_client::<cubecl::wgpu::SpirvCompiler>(cubecl::wgpu::WgpuBackend::Vulkan)
    }

    /// Direct MSL compilation with wgpu managing Metal execution.
    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub fn metal() -> Self {
        wgpu_client::<cubecl::wgpu::MslCompiler>(cubecl::wgpu::WgpuBackend::Metal)
    }

    /// CubeCL's independent Metal runtime.
    #[cfg(all(feature = "metal-native", target_os = "macos"))]
    pub fn metal_native() -> Self {
        Self::from_client(cubecl::metal::MetalRuntime::client(&Default::default()))
    }

    #[cfg(feature = "cpu")]
    pub fn cpu() -> Self {
        Self::from_client(cubecl::cpu::CpuRuntime::client(&Default::default()))
    }

    #[cfg(feature = "cuda")]
    pub fn cuda() -> Self {
        Self::from_client(cubecl::cuda::CudaRuntime::client(&Default::default()))
    }

    #[cfg(feature = "hip")]
    pub fn hip() -> Self {
        Self::from_client(cubecl::hip::HipRuntime::client(&Default::default()))
    }
}

#[cfg(feature = "wgpu")]
fn wgpu_client<C: cubecl::wgpu::WgpuCompiler>(api: cubecl::wgpu::WgpuBackend) -> RuntimeContext {
    let device = cubecl::wgpu::WgpuDevice::default().on(api);
    RuntimeContext::from_client(cubecl::wgpu::WgpuRuntime::<C>::client(&device))
}

pub(super) fn select(name: &str) -> Result<fn() -> RuntimeContext> {
    match name {
        "metal" | "metal-native" if !cfg!(target_os = "macos") => {
            Err(Error::Runtime(format!("DRIP_BACKEND={name} requires macOS")))
        }
        "dx12" if !cfg!(target_os = "windows") => {
            Err(Error::Runtime("DRIP_BACKEND=dx12 requires Windows".into()))
        }
        #[cfg(feature = "wgpu")]
        "wgpu" | "wgsl" => Ok(RuntimeContext::wgpu),
        #[cfg(all(feature = "wgpu", target_os = "windows"))]
        "dx12" => Ok(RuntimeContext::wgpu),
        #[cfg(feature = "vulkan")]
        "vulkan" => Ok(RuntimeContext::vulkan),
        #[cfg(all(feature = "metal", target_os = "macos"))]
        "metal" => Ok(RuntimeContext::metal),
        #[cfg(all(feature = "metal-native", target_os = "macos"))]
        "metal-native" => Ok(RuntimeContext::metal_native),
        #[cfg(feature = "cpu")]
        "cpu" => Ok(RuntimeContext::cpu),
        #[cfg(feature = "cuda")]
        "cuda" => Ok(RuntimeContext::cuda),
        #[cfg(feature = "hip")]
        "hip" => Ok(RuntimeContext::hip),
        name if matches!(name, "wgpu" | "wgsl" | "dx12") => missing_feature(name, "wgpu"),
        name if matches!(name, "vulkan" | "metal" | "metal-native" | "cpu" | "cuda" | "hip") => {
            missing_feature(name, name)
        }
        _ => Err(Error::Runtime(format!(
            "unknown DRIP_BACKEND={name:?}; expected wgpu, wgsl, dx12, vulkan, metal, metal-native, cuda, hip or cpu"
        ))),
    }
}

fn missing_feature(name: &str, feature: &str) -> Result<fn() -> RuntimeContext> {
    Err(Error::Runtime(format!(
        "DRIP_BACKEND={name} requires a build with --features drip/{feature}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_checks_build_features_without_initializing_devices() {
        for (name, enabled) in [
            ("wgpu", cfg!(feature = "wgpu")),
            ("wgsl", cfg!(feature = "wgpu")),
            ("vulkan", cfg!(feature = "vulkan")),
            ("cpu", cfg!(feature = "cpu")),
            ("cuda", cfg!(feature = "cuda")),
            ("hip", cfg!(feature = "hip")),
            ("metal", cfg!(all(feature = "metal", target_os = "macos"))),
            ("metal-native", cfg!(all(feature = "metal-native", target_os = "macos"))),
            ("dx12", cfg!(all(feature = "wgpu", target_os = "windows"))),
        ] {
            assert_eq!(select(name).is_ok(), enabled, "{name}");
        }
        for name in ["", "CPU", "auto", "unknown"] {
            assert!(select(name).err().unwrap().to_string().contains("unknown DRIP_BACKEND"));
        }
        #[cfg(not(feature = "cpu"))]
        assert!(select("cpu").err().unwrap().to_string().contains("--features drip/cpu"));
        #[cfg(not(target_os = "macos"))]
        assert!(select("metal-native").err().unwrap().to_string().contains("requires macOS"));
    }
}
