//! One explicit compute device per evaluator. Host nodes remain ordinary Rust.
use crate::{Error, Result};
use cubecl::{__private::Runtime, prelude::*};

/// Processing request shared by every node and its contract. A scale of one
/// requests full detail. Demosaic applies this spatial reduction internally;
/// other nodes may preserve their input sampling or define their own policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalContext {
    /// Requested spatial reduction factor. One means full detail.
    pub scale: u32,
}
impl Default for GlobalContext {
    fn default() -> Self {
        Self { scale: 1 }
    }
}
impl GlobalContext {
    pub fn validate(&self) -> Result<()> {
        if !(1..=256).contains(&self.scale) {
            return Err(Error::Contract("global scale must be 1..=256".into()));
        }
        Ok(())
    }
}

pub struct RuntimeContext {
    pub(crate) client: Option<Client>,
}

impl RuntimeContext {
    /// Select computation with DRIP_BACKEND: wgpu (default), cpu or cuda.
    /// CPU and CUDA require their corresponding Cargo features. An explicit
    /// unavailable backend is an error; this does not change GUI rendering.
    pub fn from_env() -> Result<Self> {
        let name = match std::env::var("DRIP_BACKEND") {
            Ok(name) => name,
            Err(std::env::VarError::NotPresent) => "wgpu".into(),
            Err(_) => return Err(Error::Runtime("DRIP_BACKEND must be valid UTF-8".into())),
        };
        Ok(backend(&name)?())
    }

    pub fn host() -> Self {
        Self { client: None }
    }

    pub fn wgpu() -> Self {
        Self::from_client(cubecl::wgpu::WgpuRuntime::<cubecl::wgpu::AutoCompiler>::client(
            &Default::default(),
        ))
    }

    /// Adopt an explicitly initialized compute client. Device identity belongs
    /// to this context; port buffers are allocated by its evaluator.
    pub fn from_client(client: Client) -> Self {
        Self { client: Some(client) }
    }

    #[cfg(feature = "cpu")]
    pub fn cpu() -> Self {
        Self::from_client(cubecl::cpu::CpuRuntime::client(&Default::default()))
    }

    #[cfg(feature = "cuda")]
    pub fn cuda() -> Self {
        Self::from_client(cubecl::cuda::CudaRuntime::client(&Default::default()))
    }

    pub(crate) fn client(&self) -> Result<&Client> {
        self.client.as_ref().ok_or_else(|| Error::Runtime("no compute runtime is bound".into()))
    }

    pub(crate) fn finish(&self) -> Result<()> {
        if let Some(client) = &self.client {
            pollster::block_on(client.sync()).map_err(|e| Error::Runtime(e.to_string()))?;
        }
        Ok(())
    }
}

fn backend(name: &str) -> Result<fn() -> RuntimeContext> {
    match name {
        "wgpu" => Ok(RuntimeContext::wgpu),
        #[cfg(feature = "cpu")]
        "cpu" => Ok(RuntimeContext::cpu),
        #[cfg(feature = "cuda")]
        "cuda" => Ok(RuntimeContext::cuda),
        name if matches!(name, "cpu" | "cuda") => Err(Error::Runtime(format!(
            "DRIP_BACKEND={name} requires a build with --features drip/{name}"
        ))),
        _ => Err(Error::Runtime(format!(
            "unknown DRIP_BACKEND={name:?}; expected wgpu, cpu or cuda"
        ))),
    }
}

pub(crate) fn dispatch_dims(elements: usize) -> (CubeCount, CubeDim) {
    let groups = elements.div_ceil(256);
    let x = groups.min(65535);
    (CubeCount::Static(x as u32, groups.div_ceil(x) as u32, 1), CubeDim::new_1d(256))
}

/// Dispatch and scratch access supplied by evaluation. Nodes use the client to
/// launch kernels; evaluator-owned port transfers are a convention, not a
/// restriction on CubeCL's client API.
pub struct KernelContext<'a> {
    pub global: &'a GlobalContext,
    pub(crate) runtime: &'a RuntimeContext,
}
impl KernelContext<'_> {
    #[doc(hidden)]
    pub fn allocate<P: crate::ports::Port>(
        &self,
        desc: &<P::Payload as crate::payload::Payload>::Desc,
    ) -> Result<P::Storage> {
        P::allocate(desc, self.runtime)
    }

    pub fn client(&self) -> Result<&Client> {
        self.runtime.client()
    }

    /// Algorithm-local scratch allocated by the runtime. It has no port meaning.
    pub fn scratch_f32(&self, elements: usize) -> Result<crate::payload::F32Buffer<()>> {
        let bytes = elements
            .checked_mul(4)
            .filter(|&n| n > 0)
            .ok_or_else(|| Error::Runtime("invalid scratch extent".into()))?;
        let handle = self.client()?.empty(bytes);
        Ok(crate::payload::F32Buffer::new(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_selection_requires_an_available_explicit_choice() {
        assert!(backend("wgpu").is_ok());
        assert_eq!(backend("cpu").is_ok(), cfg!(feature = "cpu"));
        assert_eq!(backend("cuda").is_ok(), cfg!(feature = "cuda"));
        for name in ["", "CPU", "auto", "unknown"] {
            assert!(backend(name).err().unwrap().to_string().contains("unknown DRIP_BACKEND"));
        }
        #[cfg(not(feature = "cpu"))]
        assert!(backend("cpu").err().unwrap().to_string().contains("--features drip/cpu"));
    }
}
