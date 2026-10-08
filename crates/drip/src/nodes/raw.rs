//! `raw.read`: decodes a raw file into a normalized mosaic.

use std::sync::Arc;

use drip_raw::Raw;

use crate::image::{Camera, Cfa, Mosaic, RawMetadata};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;

#[derive(crate::Parameters)]
pub struct RawSource {
    #[param(ParamKind::Path { output: false })]
    #[external]
    path: Option<std::path::PathBuf>,
}

/// Decode a Bayer RAW, subtract its black levels and normalize using sensor saturation.
///
/// Output is a CPU Mosaic in black-subtracted sensor response coordinates;
/// clipping references and as-shot gains remain attached. A missing or singular
/// color characterization does not prevent sensor-domain processing.
/// RAW metadata is also available as a separate output.
#[crate::node(kind = READ, id = "raw.read", category = "raw", name = "RAW", outputs = ["mosaic", "metadata"])]
fn read(
    #[params] p: RawSource,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<Mosaic>, Arc<RawMetadata>), KernelError> {
    let path = p.path.as_deref().ok_or(KernelError::Incomplete("no raw file chosen"))?;
    let source = ctx.resources().load(path, |path| {
        let raw = drip_raw::decode(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Source { mosaic: Arc::new(normalize(&raw)?), metadata: Arc::new(raw.metadata) })
    })?;
    Ok((source.mosaic.clone(), source.metadata.clone()))
}

// The source snapshot owns normalization too: parameter edits downstream must
// not rescan the RAW on the CPU. Reloading resources replaces this snapshot;
// preview scale belongs to demosaic and does not create more source copies.
struct Source {
    mosaic: Arc<Mosaic>,
    metadata: Arc<RawMetadata>,
}

/// Subtracts black and scales sensor saturation to 1, cropping partial Bayer
/// cells at the edges.
///
/// Black follows LibRaw's model as it stands right after unpacking: a common
/// level, a per-color offset and a repeating pattern. As in dcraw, every site
/// is divided by one denominator, saturation minus the black common to all
/// sites as LibRaw's `adjust_bl()` derives it \[1\]; per-color denominators would
/// act as a hidden white balance.
///
/// \[1\] LibRaw, `adjust_bl()` and `subtract_black_internal()`, LibRaw 0.22.
pub fn normalize(raw: &Raw) -> Result<Mosaic, String> {
    if raw.width < 2 || raw.height < 2 || raw.width.checked_mul(raw.height) != Some(raw.data.len())
    {
        return Err("raw dimensions do not describe complete Bayer data".into());
    }
    let colors: Vec<_> = raw.cfa.iter().flatten().map(|&c| if c == 3 { 1 } else { c }).collect();
    if !matches!(colors.as_slice(), [0, 1, 1, 2] | [1, 0, 2, 1] | [1, 2, 0, 1] | [2, 1, 1, 0]) {
        return Err("a 2 × 2 Bayer pattern is required".into());
    }
    if raw.pattern.height.checked_mul(raw.pattern.width) != Some(raw.pattern.values.len()) {
        return Err("black pattern dimensions do not match its values".into());
    }
    // Like LibRaw, use the first green's multiplier when the second has none.
    let g2 = if raw.as_shot[3] > 0.0 { raw.as_shot[3] } else { raw.as_shot[1] };
    let white_balance =
        [raw.as_shot[0], raw.as_shot[1], raw.as_shot[2], g2].map(|m| m / raw.as_shot[1]);
    if !white_balance.iter().all(|m| m.is_finite() && *m > 0.0) {
        return Err("the raw has no usable as-shot white balance".into());
    }

    let black = |row: usize, col: usize| {
        u64::from(raw.black)
            + u64::from(raw.channel_black[raw.cfa[row % 2][col % 2] as usize])
            + u64::from(raw.pattern.at(row, col))
    };
    let small_pattern =
        (1..=2).contains(&raw.pattern.height) && (1..=2).contains(&raw.pattern.width);
    let common = if small_pattern {
        // adjust_bl() folds patterns of at most 2 × 2 into the per-color offsets
        // first, so the common part is the smallest black of one Bayer cell.
        (0..4).map(|i| black(i / 2, i % 2)).min().expect("4 sites")
    } else {
        u64::from(raw.black)
            + u64::from(*raw.channel_black.iter().min().expect("4 channels"))
            + u64::from(raw.pattern.values.iter().min().copied().unwrap_or(0))
    };
    if u64::from(raw.maximum) <= common {
        return Err(format!("saturation {} is not above black {common}", raw.maximum));
    }
    let gain = 1.0 / (u64::from(raw.maximum) - common) as f32;

    let (width, height) = (raw.width / 2 * 2, raw.height / 2 * 2);
    let mut data = Vec::with_capacity(width * height);
    let mut white = [f32::INFINITY; 4];
    for row in 0..height {
        for col in 0..width {
            let black = black(row, col) as f32;
            let color = raw.cfa[row % 2][col % 2] as usize;
            // A patterned black can vary within one color. Use its lowest
            // saturation level so no genuinely clipped site is missed.
            white[color] = white[color].min((raw.maximum as f32 - black) * gain);
            data.push((raw.data[row * raw.width + col] as f32 - black) * gain);
        }
    }
    if white[3].is_infinite() {
        white[3] = white[1];
    }
    if !white.iter().all(|&v| v.is_finite() && v > 0.0) {
        return Err("the raw has no usable per-color saturation levels".into());
    }
    let cfa = Cfa { size: 2, colors: raw.cfa.concat() };
    let camera = Arc::new(Camera { xyz_to_cam: raw.xyz_to_cam, white_balance });
    Mosaic::try_new(
        Arc::new(crate::image::RawMat::from_samples(width, height, 1, data)),
        crate::image::SensorMosaic { cfa, white, camera },
    )
    .map_err(|error| error.to_string())
}
