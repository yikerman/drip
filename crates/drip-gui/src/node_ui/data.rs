//! Host images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Error, Result,
    eval::InputValues,
    node::data::{Camera, Color, HostBuffer, Interpretation, Rgb as PayloadRgb},
    ports::Cpu,
    resource::Resources,
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub requested_scale: u32,
    pub color_space: String,
    pub pixels: Arc<HostBuffer<[f32; 3]>>,
}

/// Names the source coordinates for diagnostic views without converting pixels.
pub trait DisplayInterpretation: Interpretation {
    fn label(&self) -> String;
}
impl DisplayInterpretation for Camera {
    fn label(&self) -> String {
        "Camera RGB".into()
    }
}
impl DisplayInterpretation for Color {
    fn label(&self) -> String {
        match self.space.as_str() {
            "rec2020-d65" => "Rec.2020 / D65".into(),
            other => other.into(),
        }
    }
}
#[derive(Default)]
pub struct PrepareContext {
    pub resources: Resources,
    pub level: u8,
}
impl PrepareContext {
    pub fn image<I: DisplayInterpretation>(&self, inputs: &InputValues) -> Result<Arc<Rgb>> {
        let value = inputs.get("image").ok_or_else(|| Error::Contract("missing image".into()))?;
        let (desc, _) = value.get::<Cpu<PayloadRgb<I>>>()?;
        let pixels = value.shared::<Cpu<PayloadRgb<I>>>()?;
        Ok(Arc::new(Rgb {
            width: desc.extent.width as usize,
            height: desc.extent.height as usize,
            requested_scale: 1 << self.level,
            color_space: desc.interpretation.label(),
            pixels,
        }))
    }
}
