//! Output capability selection and platform tagging. Pixel conversion lives in `compositor`.

use wgpu::{SurfaceCapabilities, SurfaceColorSpace as Space, TextureFormat};

use super::{CANVAS, wayland};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Surface encoding and the lifetime of any application-owned declaration.
pub(super) struct Presentation {
    pub output: Output,
    _description: Option<wayland::Description>,
}

impl Presentation {
    pub fn new(window: &Window, caps: &SurfaceCapabilities) -> Result<Self, String> {
        let is_wayland = matches!(
            window.window_handle().map_err(|e| e.to_string())?.as_raw(),
            RawWindowHandle::Wayland(_)
        );
        let (preferred, description, fallback) = if is_wayland {
            // Never attach our description unless passthrough prevents the driver
            // from attaching its own color-management object to this window.
            let described =
                if caps.color_spaces(CANVAS).contains(wgpu::SurfaceColorSpaces::PASS_THROUGH) {
                    wayland::Description::new(window)
                } else {
                    Err("FP16 passthrough unavailable".into())
                };
            match described {
                Ok(description) => (Space::PassThrough, Some(description), None),
                // Driver-owned Wayland scRGB has inconsistent reference white.
                Err(reason) => (Space::Srgb, None, Some(reason)),
            }
        } else {
            (Space::ExtendedSrgbLinear, None, None)
        };
        let output = Output::choose(caps, preferred).ok_or("no usable surface color space")?;
        if !output.wide_gamut() {
            log::warn!(
                "window={:?} using sRGB: {}",
                window.id(),
                fallback.as_deref().unwrap_or("FP16 extended-linear output unavailable")
            );
        }
        Ok(Self { output, _description: description })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Output {
    pub format: TextureFormat,
    pub color_space: Space,
}

impl Output {
    /// Try the backend's preferred FP16 encoding, otherwise use bounded sRGB.
    pub fn choose(caps: &SurfaceCapabilities, preferred: Space) -> Option<Self> {
        if preferred != Space::Srgb
            && caps.color_spaces(CANVAS).contains(preferred.to_color_spaces()?)
        {
            return Some(Self { format: CANVAS, color_space: preferred });
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
            Space::PassThrough | Space::ExtendedSrgbLinear => "linear",
            Space::Srgb if self.format.is_srgb() => "srgb_linear",
            Space::Srgb => "srgb",
            _ => unreachable!("only pass-through, extended-sRGB and sRGB outputs are selected"),
        }
    }
}
