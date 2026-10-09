//! Host images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Error, Result,
    eval::InputValues,
    node::data::{Interpretation, Rgb as PayloadRgb},
    ports::Cpu,
    resource::Resources,
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub requested_scale: u32,
    pub pixels: Arc<Vec<[f32; 3]>>,
}
#[derive(Default)]
pub struct PrepareContext {
    pub resources: Resources,
    pub level: u8,
}
impl PrepareContext {
    pub fn image<I: Interpretation>(&self, inputs: &InputValues) -> Result<Arc<Rgb>> {
        let value = inputs.get("image").ok_or_else(|| Error::Contract("missing image".into()))?;
        let (desc, _) = value.get::<Cpu<PayloadRgb<I>>>()?;
        let pixels = value.shared::<Cpu<PayloadRgb<I>>>()?;
        Ok(Arc::new(Rgb {
            width: desc.extent.width as usize,
            height: desc.extent.height as usize,
            requested_scale: 1 << self.level,
            pixels,
        }))
    }
}
