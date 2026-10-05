//! User-facing descriptions. Port contracts are rendered separately from the
//! kernel signature, so these entries describe operations and assumptions.

use drip::node::NodeKind;
use drip::nodes;

pub(super) struct Documentation {
    pub description: &'static str,
    pub result: Option<(&'static str, &'static str)>,
    pub reference: Option<(&'static str, &'static str)>,
}

impl Documentation {
    const fn new(description: &'static str) -> Self {
        Self { description, result: None, reference: None }
    }

    const fn result(mut self, kind: &'static str, name: &'static str) -> Self {
        self.result = Some((kind, name));
        self
    }

    const fn reference(mut self, name: &'static str, url: &'static str) -> Self {
        self.reference = Some((name, url));
        self
    }
}

pub(super) fn of(kind: &NodeKind) -> Option<&'static Documentation> {
    static DOCS: &[(&NodeKind, Documentation)] = &[
        (
            &nodes::READ,
            Documentation::new(
                "Decode a Bayer RAW, subtract its black levels and normalize using sensor saturation. \
                 Camera characterization and as-shot white balance remain attached to the mosaic.",
            ),
        ),
        (
            &nodes::WHITE_BALANCE,
            Documentation::new(
                "Multiply Bayer samples by the as-shot gains relative to the first green channel. \
                 Apply once before highlight reconstruction.",
            ),
        ),
        (
            &nodes::HIGHLIGHTS,
            Documentation::new(
                "Reconstruct clipped Bayer samples with inpaint opposed. Assumes white-balanced input. \
                 Threshold is a fraction of each channel's saturation level. Runs before preview reduction.",
            )
            .reference(
                "darktable: inpaint opposed",
                "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/highlight-reconstruction/",
            ),
        ),
        (
            &nodes::RCD,
            Documentation::new(
                "Bayer demosaicing with Ratio Corrected Demosaicing (RCD) and a bilinear border. \
                 Preview reduction precedes interpolation.",
            )
            .reference("RCD algorithm", "https://github.com/LuisSR/RCD-Demosaicing"),
        ),
        (
            &nodes::BIN_2X2,
            Documentation::new(
                "Average each 2 × 2 Bayer cell into one RGB pixel, combining its two greens. \
                 Output width and height are halved after preview reduction.",
            ),
        ),
        (
            &nodes::CAMERA_TO_REC2020,
            Documentation::new(
                "Transform white-balanced camera RGB to Rec.2020/D65 using the camera matrix. \
                 Neutral camera RGB (1, 1, 1) maps to neutral output.",
            ),
        ),
        (
            &nodes::EXPOSURE,
            Documentation::new(
                "Applies exponential exposure adjustment (x * 2^ev) in linear space.",
            ),
        ),
        (
            &nodes::SIGMOID,
            Documentation::new(
                "Map scene values to display values with a smooth S-curve. \
                 Assumes middle grey at 0.18 and keeps it fixed. Black is 0 and the curve approaches white at 1.",
            )
            .reference(
                "darktable: sigmoid",
                "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/sigmoid/",
            ),
        ),
        (
            &nodes::PREVIEW,
            Documentation::new(
                "Show linear Rec.2020 through the display path. Scene input is shown without tone mapping. \
                 Display conversion does not change graph values.",
            )
            .result("view", "image preview"),
        ),
        (
            &nodes::HISTOGRAM,
            Documentation::new(
                "Count each channel in log2(x) bins, with 0 EV at x = 1. \
                 Out-of-range values accumulate in the end bins. Scale selects linear or logarithmic count display.",
            )
            .result("view", "channel histogram"),
        ),
        (
            &nodes::WAVEFORM,
            Documentation::new(
                "Plot channel values by image column. Levels are log2(x), with 0 EV at x = 1. \
                 Brightness indicates sample count.",
            )
            .result("view", "channel waveform"),
        ),
        (
            &nodes::VECTORSCOPE,
            Documentation::new(
                "Plot CIE u'v' chromaticity relative to D65. Markers show the input color space's primaries. \
                 Black is omitted. Negative channels are clipped for this view only.",
            )
            .result("view", "chromaticity scope")
            .reference("CIE: u'v' chromaticity", "https://cie.co.at/eilvterm/17-23-073"),
        ),
        (
            &nodes::TIFF,
            Documentation::new(
                "Export at full detail through the selected RGB ICC profile, including its transfer encoding. \
                 Choose 16-bit integer or 32-bit float samples.",
            )
            .result("writes", "TIFF file"),
        ),
    ];
    DOCS.iter().find(|(node, _)| *node == kind).map(|(_, doc)| doc)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_builtin_node_has_user_documentation() {
        for kind in drip::nodes::registry().kinds() {
            assert!(super::of(kind).is_some(), "{}", kind.name);
        }
    }
}
