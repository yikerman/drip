use crate::{
    Error, Result,
    node::{
        camera,
        color::{self, Mat3},
        data::*,
    },
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
use std::sync::Arc;

#[derive(Clone, Default, crate::Choice)]
pub enum Illuminant {
    #[default]
    #[choice("as_shot")]
    #[label("As shot")]
    AsShot,
    #[choice("custom")]
    #[label("Custom white point")]
    Custom(WhitePoint),
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct WhitePoint {
    /// CIE 1931 x of the source illuminant; both whites are normalized to Y = 1.
    #[param(crate::param::ParamKind::Float { min: 0.0, max: 1.0, default: color::D65[0] })]
    #[label("x")]
    pub x: f64,
    /// CIE 1931 y of the source illuminant.
    #[param(crate::param::ParamKind::Float { min: 0.0, max: 1.0, default: color::D65[1] })]
    #[label("y")]
    pub y: f64,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Infer the scene white from CaptureMetadata, or supply its chromaticity.
    #[param(Illuminant::AsShot.schema())]
    pub illuminant: Illuminant,
}

// CAT16 response matrix [21]. Full von Kries adaptation follows Colour 0.4.7's
// matrix_chromatic_adaptation_VonKries(..., transform="CAT16"). Original Rust
// implementation of the published equations; no Python code is copied.
const CAT16: Mat3 = [
    [0.401288, 0.650173, -0.051461],
    [-0.250268, 1.204414, 0.045854],
    [-0.002079, 0.048952, 0.953127],
];

fn xyz([x, y]: [f64; 2]) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Full CAT16 adaptation between equal-luminance CIE 1931 xy whites.
/// White responses must be finite and positive; image samples remain unrestricted.
fn adaptation(source: [f64; 2], target: [f64; 2]) -> Result<Mat3> {
    let source = color::apply(&CAT16, xyz(source));
    let target = color::apply(&CAT16, xyz(target));
    if source.iter().chain(&target).any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(Error::Contract(
            "CAT16 white must have finite, positive cone responses".into(),
        ));
    }
    let scaled = std::array::from_fn(|r| CAT16[r].map(|v| v * target[r] / source[r]));
    Ok(color::mul(&color::inverse(&CAT16), &scaled))
}

/// Working-RGB adaptation from the source white to D65.
fn matrix(p: &Settings, metadata: Option<&drip_raw::Metadata>) -> Result<[[f32; 3]; 3]> {
    let source = match &p.illuminant {
        Illuminant::Custom(white) => [white.x, white.y],
        Illuminant::AsShot => camera::as_shot_white(metadata.ok_or_else(|| {
            Error::Contract("as-shot adaptation needs the capture metadata".into())
        })?)?,
    };
    let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
    let matrix = color::to_f32(&color::mul(
        &color::inverse(&rgb_to_xyz),
        &color::mul(&adaptation(source, color::D65)?, &rgb_to_xyz),
    ));
    if matrix.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Error::Contract("CAT16 matrix exceeds finite f32 coefficients".into()));
    }
    Ok(matrix)
}

fn validate(p: &Settings) -> Result<()> {
    match p.illuminant {
        Illuminant::Custom(_) => matrix(p, None).map(drop),
        Illuminant::AsShot => Ok(()),
    }
}
fn contract(
    _: &GlobalContext,
    p: &Settings,
    image: &RgbDesc,
    metadata: Option<&Arc<drip_raw::Metadata>>,
) -> Result<(RgbDesc,)> {
    // Unconnected metadata is incomplete wiring, reported when evaluating As shot.
    if let Some(metadata) = metadata {
        matrix(p, Some(metadata))?;
    }
    Ok((image.clone(),))
}

/// Adapt a scene illuminant to D65 using full CAT16 chromatic adaptation.
/// Input and output are linear Rec.2020/D65 RGB. As shot maps the as-shot camera
/// neutral through the capture's camera matrix to infer the source white.
/// Custom white point supplies CIE 1931 xy without metadata.
/// The RAW template cancels as-shot gains at camera conversion before this node.
/// Place before rendering; on already-balanced RGB, a custom source white is a
/// creative reinterpretation. Signed/HDR samples are preserved without clipping.
#[crate::node(id="color.chromatic-adaptation", name="Chromatic adaptation", category="Color", contract=contract,
    references=[("CAT16", "https://doi.org/10.1002/col.22131"),
        ("Colour full adaptation", "https://colour.readthedocs.io/en/v0.4.7/generated/colour.adaptation.matrix_chromatic_adaptation_VonKries.html")])]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<Rgb>, Rec2020>,
    metadata: Option<Read<'_, Cpu<CaptureMetadata>>>,
    output: Write<'_, Device<Rgb>, Rec2020>,
) -> Result<()> {
    let values = matrix(p, metadata.as_ref().map(|m| &**m.data))?;
    let coefficients = ctx.constants(values.as_flattened())?;
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    super::shared_kernel::matrix::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        coefficients.argument(),
        output.data.argument(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eval::Evaluator, graph::Dag, runtime::RuntimeContext};

    fn custom(xy: [f64; 2]) -> Settings {
        Settings { illuminant: Illuminant::Custom(WhitePoint { x: xy[0], y: xy[1] }) }
    }

    #[test]
    fn white_mapping_inverse_and_colour_reference() {
        let a = [0.44757, 0.40745];
        let matrix = adaptation(a, color::D65).unwrap();
        // Colour 0.4.7, matrix_chromatic_adaptation_VonKries(xy_to_XYZ(A),
        // xy_to_XYZ(D65), transform="CAT16"). See THIRD_PARTY [21].
        let expected = [
            [0.950686466778631, -0.187385883906847, 0.262897668222764],
            [-0.0256671403956933, 1.03230983361068, -0.0115657309820062],
            [-0.00274218552169476, 0.0910058500023380, 2.81337816897230],
        ];
        for (actual, expected) in matrix.iter().flatten().zip(expected.iter().flatten()) {
            assert!((actual - expected).abs() < 1e-13);
        }
        for source in [a, color::D65, [0.34567, 0.35850], [0.28, 0.31]] {
            let forward = adaptation(source, color::D65).unwrap();
            let reverse = adaptation(color::D65, source).unwrap();
            for (actual, expected) in
                color::apply(&forward, xyz(source)).into_iter().zip(xyz(color::D65))
            {
                assert!((actual - expected).abs() < 1e-12);
            }
            let roundtrip = color::mul(&reverse, &forward);
            for (r, row) in roundtrip.iter().enumerate() {
                for (c, value) in row.iter().enumerate() {
                    assert!((value - if r == c { 1.0 } else { 0.0 }).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn contracts_and_metadata_failures() {
        for white in [[0.3, 0.0], [f64::NAN, 0.3], [0.3, f64::INFINITY], [0.9, 0.01]] {
            assert!(add(&mut Dag::new(), custom(white)).is_err());
        }
        assert!(matrix(&Settings::default(), None).is_err());
        assert!(matrix(&Settings::default(), Some(&drip_raw::Metadata::default())).is_err());
        let desc = RgbDesc {
            extent: Extent { width: 1, height: 1 },
            coordinates: Coordinates::Space(REC2020_D65),
        };
        assert!(contract(&Default::default(), &custom(color::D65), &desc, None).is_ok());
        let unusable = Arc::new(drip_raw::Metadata::default());
        assert!(
            contract(&Default::default(), &Settings::default(), &desc, Some(&unusable)).is_err()
        );
    }

    fn device_reference(runtime: RuntimeContext) {
        let mut dag = Dag::new();
        let desc = RgbDesc {
            extent: Extent { width: 4, height: 1 },
            coordinates: Coordinates::Space(REC2020_D65),
        };
        let input = crate::node::source::<Rgb>(
            &mut dag,
            desc,
            vec![[0.2, 0.5, 0.8], [-0.1, 0.2, 8.], [0.; 3], [16., -2., 0.5]],
        )
        .unwrap();
        let cat = add(&mut dag, custom([0.44757, 0.40745])).unwrap();
        dag.connect(input, cat.image).unwrap();
        let output = Evaluator::new(runtime)
            .evaluate::<Cpu<Rgb>>(&dag, &Default::default(), cat.output)
            .unwrap();
        // Colour 0.4.7: RGB_to_XYZ -> CAT16 A-to-D65 -> XYZ_to_RGB, BT.2020.
        let expected = [
            [0.0150916964441549, 0.432812976444011, 2.31325856901239],
            [-0.440684843548889, -1.11172371305315, 22.5979969299213],
            [0., 0., 0.],
            [14.2259813421445, -1.65111855363381, 1.52244860850617],
        ];
        for (actual, expected) in output.data.iter().flatten().zip(expected.iter().flatten()) {
            assert!((f64::from(*actual) - expected).abs() < 5e-6, "{actual} != {expected}");
        }
        assert_eq!(output.desc.coordinates, Coordinates::Space(REC2020_D65));
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn cpu_colour_reference() {
        device_reference(RuntimeContext::cpu());
    }

    #[test]
    #[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
    fn selected_backend_colour_reference() {
        device_reference(RuntimeContext::from_env().unwrap());
    }
}
