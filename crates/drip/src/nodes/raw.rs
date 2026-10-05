//! `raw.read`: decodes a raw file into a normalized mosaic (DESIGN C2, E5).

use std::sync::Arc;

use drip_libraw::Raw;

use crate::color::{self, D65, REC2020};
use crate::node::{EvalContext, Evaluated, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::value::{Camera, Cfa, Mosaic, PortType, Value};

pub static READ: NodeKind = NodeKind {
    name: "raw.read",
    label: "raw",
    params: &[ParamSpec::new("path", ParamKind::Path { output: false }).external()],
    inputs: &[],
    outputs: &[
        OutputSpec { name: "mosaic", ty: PortType::Mosaic },
        OutputSpec { name: "metadata", ty: PortType::RawMetadata },
    ],
    eval: read,
    actions: &[],
};

fn read(p: Params, _: &[Value], ctx: &EvalContext) -> Result<Evaluated, String> {
    let path = p.path("path").ok_or("no raw file chosen")?;
    let raw = ctx.resources().load(path, |path| {
        drip_libraw::decode(path).map_err(|e| format!("{}: {e}", path.display()))
    })?;
    let mosaic = normalize(&raw)?;
    let outputs =
        vec![Value::Mosaic(Arc::new(mosaic)), Value::RawMetadata(Arc::new(raw.metadata.clone()))];
    Ok(Evaluated { outputs, view: None })
}

/// Subtracts black and scales sensor saturation to 1, cropping partial Bayer
/// cells at the edges.
///
/// Black follows LibRaw's model as it stands right after unpacking: a common
/// level, a per-color offset and a repeating pattern. As in dcraw, every site
/// is divided by one denominator, saturation minus the black common to all
/// sites as LibRaw's `adjust_bl()` derives it [1]; per-color denominators would
/// act as a hidden white balance.
///
/// [1] LibRaw, `adjust_bl()` and `subtract_black_internal()`, LibRaw 0.22.
pub fn normalize(raw: &Raw) -> Result<Mosaic, String> {
    // Like LibRaw, use the first green's multiplier when the second has none.
    let g2 = if raw.as_shot[3] > 0.0 { raw.as_shot[3] } else { raw.as_shot[1] };
    let white_balance =
        [raw.as_shot[0], raw.as_shot[1], raw.as_shot[2], g2].map(|m| m / raw.as_shot[1]);
    if !white_balance.iter().all(|m| m.is_finite() && *m > 0.0) {
        return Err("the raw has no usable as-shot white balance".into());
    }
    if color::camera_to_rgb(&color::to_f64(&raw.xyz_to_cam), &color::rgb_to_xyz(REC2020, D65))
        .is_none()
    {
        return Err("the camera's color matrix is missing or singular".into());
    }

    let black = |row: usize, col: usize| {
        raw.black + raw.channel_black[raw.cfa[row % 2][col % 2] as usize] + raw.pattern.at(row, col)
    };
    let small_pattern =
        (1..=2).contains(&raw.pattern.height) && (1..=2).contains(&raw.pattern.width);
    let common = if small_pattern {
        // adjust_bl() folds patterns of at most 2 × 2 into the per-color offsets
        // first, so the common part is the smallest black of one Bayer cell.
        (0..4).map(|i| black(i / 2, i % 2)).min().expect("4 sites")
    } else {
        raw.black
            + raw.channel_black.iter().min().expect("4 channels")
            + raw.pattern.values.iter().min().copied().unwrap_or(0)
    };
    if raw.maximum <= common {
        return Err(format!("saturation {} is not above black {common}", raw.maximum));
    }
    let gain = 1.0 / (raw.maximum - common) as f32;

    let (width, height) = (raw.width / 2 * 2, raw.height / 2 * 2);
    let mut data = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            data.push((raw.data[row * raw.width + col] as f32 - black(row, col) as f32) * gain);
        }
    }
    let cfa = Cfa { size: 2, colors: raw.cfa.concat() };
    let camera = Arc::new(Camera { xyz_to_cam: raw.xyz_to_cam, white_balance });
    Ok(Mosaic { width, height, scale: 1, cfa, data, camera })
}

/// Halves a Bayer mosaic by averaging four sites of each phase. Only whole
/// 4 × 4 input cells contribute, so every output remains a complete Bayer cell.
pub fn downsample(m: &Mosaic) -> Mosaic {
    let (width, height) = (m.width / 4 * 2, m.height / 4 * 2);
    let mut data = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            let (r, c) = (row / 2 * 4 + row % 2, col / 2 * 4 + col % 2);
            let i = r * m.width + c;
            data.push(
                (m.data[i] + m.data[i + 2] + m.data[i + 2 * m.width] + m.data[i + 2 * m.width + 2])
                    * 0.25,
            );
        }
    }
    Mosaic { width, height, scale: m.scale * 2, data, cfa: m.cfa.clone(), camera: m.camera.clone() }
}
