//! Images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Error, Result,
    eval::InputValues,
    node::data::{DeviceBuffer, HostBuffer, Rgb, RgbDesc},
    ports::{Cpu, Device},
    resource::Resources,
};
use std::sync::Arc;

/// Host pixels prepared for presentation, with the labels views show.
#[derive(Debug, Clone, PartialEq)]
pub struct HostImage {
    pub width: usize,
    pub height: usize,
    pub requested_scale: u32,
    pub color_space: String,
    pub pixels: Arc<HostBuffer<[f32; 3]>>,
}

#[derive(Default)]
pub struct PrepareContext {
    pub client: Option<cubecl::prelude::Client>,
    pub resources: Resources,
    pub level: u8,
}
pub struct DeviceRgb {
    pub desc: RgbDesc,
    pub pixels: Arc<DeviceBuffer<Rgb>>,
}

impl PrepareContext {
    pub fn client(&self) -> Result<&cubecl::prelude::Client> {
        self.client.as_ref().ok_or_else(|| Error::Runtime("no compute runtime is bound".into()))
    }

    pub fn device_image(&self, inputs: &InputValues) -> Result<DeviceRgb> {
        let value = inputs.get("image").ok_or_else(|| Error::Contract("missing image".into()))?;
        let (desc, _) = value.get::<Device<Rgb>>()?;
        Ok(DeviceRgb { desc: desc.clone(), pixels: value.shared::<Device<Rgb>>()? })
    }

    pub fn image(&self, inputs: &InputValues) -> Result<Arc<HostImage>> {
        let value = inputs.get("image").ok_or_else(|| Error::Contract("missing image".into()))?;
        let (desc, _) = value.get::<Cpu<Rgb>>()?;
        Ok(Arc::new(HostImage {
            width: desc.extent.width as usize,
            height: desc.extent.height as usize,
            requested_scale: 1 << self.level,
            color_space: desc.coordinates.to_string(),
            pixels: value.shared::<Cpu<Rgb>>()?,
        }))
    }
}
