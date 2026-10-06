use wgpu::{SurfaceCapabilities, SurfaceColorSpace as Space, TextureFormat};

use super::CANVAS;

#[derive(Clone, Copy, Debug)]
pub(super) struct Output {
    pub format: TextureFormat,
    pub color_space: Space,
}

impl Output {
    pub fn choose(caps: &SurfaceCapabilities) -> Option<Self> {
        if caps.color_spaces(CANVAS).contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR) {
            return Some(Self { format: CANVAS, color_space: Space::ExtendedSrgbLinear });
        }
        // Prefer an encoded target; an sRGB texture needs shader decoding to
        // compensate for its automatic encoding on write.
        let format = caps
            .format_capabilities
            .iter()
            .filter(|f| f.color_spaces.contains(wgpu::SurfaceColorSpaces::SRGB))
            .min_by_key(|f| f.format.is_srgb())?
            .format;
        Some(Self { format, color_space: Space::Srgb })
    }

    pub fn wide_gamut(self) -> bool {
        self.color_space != Space::Srgb
    }

    pub fn fragment(self) -> &'static str {
        match self.color_space {
            Space::ExtendedSrgbLinear => "linear",
            Space::Srgb if self.format.is_srgb() => "srgb_linear",
            Space::Srgb => "srgb",
            _ => unreachable!("only extended-sRGB and sRGB outputs are selected"),
        }
    }
}
