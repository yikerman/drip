use crate::{
    Result,
    body::{Projective, Warp},
    node::data::*,
    ports::*,
    registry::tag,
    runtime::GlobalContext,
};
pub use compute::{Definition, Ports, add, definition};

#[derive(Clone, Copy, Debug, PartialEq, Eq, crate::Choice)]
pub enum Orientation {
    #[default]
    #[choice("from_metadata")]
    #[label("From metadata")]
    FromMetadata,
    #[choice("identity")]
    #[label("Identity")]
    Identity,
    #[choice("mirror_horizontal")]
    #[label("Mirror horizontal")]
    MirrorHorizontal,
    #[choice("rotate_180")]
    #[label("Rotate 180°")]
    Rotate180,
    #[choice("mirror_vertical")]
    #[label("Mirror vertical")]
    MirrorVertical,
    #[choice("transpose")]
    #[label("Transpose")]
    Transpose,
    #[choice("rotate_90_clockwise")]
    #[label("Rotate 90° clockwise")]
    Rotate90Clockwise,
    #[choice("transverse")]
    #[label("Transverse")]
    Transverse,
    #[choice("rotate_90_counterclockwise")]
    #[label("Rotate 90° counterclockwise")]
    Rotate90Counterclockwise,
}

impl Orientation {
    fn resolve(self, metadata: Option<&drip_raw::Metadata>) -> u8 {
        match self {
            Self::FromMetadata => metadata.map_or(1, |m| m.orientation),
            Self::Identity => 1,
            Self::MirrorHorizontal => 2,
            Self::Rotate180 => 3,
            Self::MirrorVertical => 4,
            Self::Transpose => 5,
            Self::Rotate90Clockwise => 6,
            Self::Transverse => 7,
            Self::Rotate90Counterclockwise => 8,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Use capture orientation, or replace it with an explicit transform.
    #[label("Orientation")]
    #[param]
    pub orientation: Orientation,
}

fn orientation_contract(
    _: &GlobalContext,
    p: &Settings,
    image: &RgbDesc,
    metadata: Option<&std::sync::Arc<drip_raw::Metadata>>,
) -> Result<(RgbDesc,)> {
    let mut output = image.clone();
    if p.orientation.resolve(metadata.map(|m| &**m)) >= 5 {
        std::mem::swap(&mut output.extent.width, &mut output.extent.height);
        std::mem::swap(&mut output.full.width, &mut output.full.height);
    }
    Ok((output,))
}

/// Rotate or reflect RGB pixels using the capture metadata's orientation.
/// Missing metadata means identity, and an explicit orientation replaces the
/// metadata's. Transpose reflects across the top-left to bottom-right
/// diagonal, and transverse across the other. Quarter turns and diagonal reflections
/// exchange width and height. Samples and color interpretation are preserved,
/// without resampling or clipping. Apply once after demosaic and color conversion.
#[crate::node(id="orientation", name="Orientation", tags = [tag::GEOMETRY], contract=orientation_contract, warp)]
pub fn compute(
    p: &Settings,
    image: Point<'_, Rgb>,
    metadata: Option<Read<'_, CaptureMetadata>>,
    _output: Point<'_, Rgb>,
) -> Result<Warp> {
    let orientation =
        p.orientation.resolve(metadata.map(|m| m.data.host().map(|m| &**m)).transpose()?);
    Ok(Warp::reindex(map(orientation, &image.desc.full)))
}

/// The output plane to the input plane of `full` size for EXIF
/// `orientation`. Orientations 2, 3, 7 and 8 mirror the input's x, and 3, 4,
/// 6 and 7 its y. From 5 on the output's axes are the input's swapped.
fn map(orientation: u8, full: &Extent) -> Projective {
    let [w, h] = [f64::from(full.width), f64::from(full.height)];
    let mirror = |flip: bool, extent: f64| if flip { (-1.0, extent) } else { (1.0, 0.0) };
    let (sx, tx) = mirror(matches!(orientation, 2 | 3 | 7 | 8), w);
    let (sy, ty) = mirror(matches!(orientation, 3 | 4 | 6 | 7), h);
    if orientation >= 5 {
        [[0.0, sx, tx], [sy, 0.0, ty], [0.0, 0.0, 1.0]]
    } else {
        [[sx, 0.0, tx], [0.0, sy, ty], [0.0, 0.0, 1.0]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        eval::Evaluator,
        graph::Dag,
        node::{
            filter::Filter,
            resample::{self, Factor, Size},
            source,
        },
        runtime::RuntimeContext,
    };
    use std::sync::Arc;

    const EXPLICIT: [Orientation; 8] = [
        Orientation::Identity,
        Orientation::MirrorHorizontal,
        Orientation::Rotate180,
        Orientation::MirrorVertical,
        Orientation::Transpose,
        Orientation::Rotate90Clockwise,
        Orientation::Transverse,
        Orientation::Rotate90Counterclockwise,
    ];

    fn image_desc() -> RgbDesc {
        RgbDesc::new(Extent { width: 3, height: 2 }, Coordinates::Space(REC2020_D65))
    }
    fn metadata(orientation: u8) -> Arc<drip_raw::Metadata> {
        Arc::new(drip_raw::Metadata { orientation, ..Default::default() })
    }

    #[test]
    fn metadata_defaults_and_explicit_override_preserve_interpretation() {
        let global = GlobalContext::default();
        let image = RgbDesc { full: Extent { width: 6, height: 4 }, ..image_desc() };
        let automatic = Settings::default();
        let (output,) = orientation_contract(&global, &automatic, &image, None).unwrap();
        assert_eq!(output.extent, image.extent);
        for (i, orientation) in EXPLICIT.into_iter().enumerate() {
            let metadata = metadata(i as u8 + 1);
            let (output,) =
                orientation_contract(&global, &automatic, &image, Some(&metadata)).unwrap();
            let expected =
                if i < 4 { image.extent.clone() } else { Extent { width: 2, height: 3 } };
            assert_eq!(output.extent, expected);
            let full = if i < 4 { image.full.clone() } else { Extent { width: 4, height: 6 } };
            assert_eq!(output.full, full);
            assert_eq!(output.coordinates, image.coordinates);
            assert_eq!(orientation.resolve(None), metadata.orientation);
            let identity = Settings { orientation: Orientation::Identity };
            let (output,) =
                orientation_contract(&global, &identity, &image, Some(&metadata)).unwrap();
            assert_eq!(output.extent, image.extent);
            assert_eq!(identity.orientation.resolve(Some(&metadata)), 1);
        }
    }

    fn permutations(runtime: RuntimeContext) {
        let evaluator = &Evaluator::new(runtime);
        // Row-major source: [0 1 2; 3 4 5]. Explicit destination orders make
        // clockwise/counterclockwise and both diagonal reflections distinct.
        fixture(
            evaluator,
            3,
            2,
            [
                &[0, 1, 2, 3, 4, 5],
                &[2, 1, 0, 5, 4, 3],
                &[5, 4, 3, 2, 1, 0],
                &[3, 4, 5, 0, 1, 2],
                &[0, 3, 1, 4, 2, 5],
                &[3, 0, 4, 1, 5, 2],
                &[5, 2, 4, 1, 3, 0],
                &[2, 5, 1, 4, 0, 3],
            ],
        );
        fixture(
            evaluator,
            1,
            3,
            [
                &[0, 1, 2],
                &[0, 1, 2],
                &[2, 1, 0],
                &[2, 1, 0],
                &[0, 1, 2],
                &[2, 1, 0],
                &[2, 1, 0],
                &[0, 1, 2],
            ],
        );
        fixture(
            evaluator,
            3,
            1,
            [
                &[0, 1, 2],
                &[2, 1, 0],
                &[2, 1, 0],
                &[0, 1, 2],
                &[0, 1, 2],
                &[0, 1, 2],
                &[2, 1, 0],
                &[2, 1, 0],
            ],
        );
        fixture(evaluator, 1, 1, [&[0]; 8]);
    }

    /// Every orientation through the graph, from metadata and explicit, alone
    /// and followed by a Resample at factor one with Lanczos-3 and with Box.
    /// Interpolating filters reproduce a reindex bit for bit. A source whose
    /// `full` exceeds its extent, as at preview scale, composes through
    /// inexact f64 scales and must still reindex exactly.
    fn fixture(evaluator: &Evaluator, width: u32, height: u32, expected: [&[usize]; 8]) {
        let extent = Extent { width, height };
        let preview = Extent { width: 2 * width + 1, height: 2 * height + 1 };
        let resample =
            |filter| resample::Settings { filter, size: Size::Factor(Factor { factor: 1.0 }) };
        let variants = [
            (extent.clone(), None),
            (preview, None),
            (extent.clone(), Some(resample(Filter::Lanczos3))),
            (extent.clone(), Some(resample(Filter::Box))),
        ];
        let samples: Vec<[f32; 3]> =
            (0..extent.pixels()).map(|i| [i as f32, -(i as f32), 8.0]).collect();
        for (full, resample) in variants {
            let image = RgbDesc { extent: extent.clone(), full, ..image_desc() };
            for (index, orientation) in EXPLICIT.into_iter().enumerate() {
                // Exercise both metadata resolution and replacement of a conflicting
                // metadata orientation, using the same exact pixel expectations.
                for mode in 0..if index == 0 { 3 } else { 2 } {
                    let p = Settings {
                        orientation: if mode == 1 {
                            orientation
                        } else {
                            Orientation::FromMetadata
                        },
                    };
                    let metadata = metadata(if mode == 0 { index as u8 + 1 } else { 1 });
                    let mut dag = Dag::new();
                    let input = source::<Rgb>(&mut dag, image.clone(), samples.clone()).unwrap();
                    let node = add(&mut dag, p).unwrap();
                    dag.connect(input, node.image).unwrap();
                    if mode != 2 {
                        let metadata =
                            source::<CaptureMetadata>(&mut dag, metadata.clone(), metadata)
                                .unwrap();
                        dag.connect(metadata, node.metadata).unwrap();
                    }
                    let mut output = node.output;
                    if let Some(settings) = resample.clone() {
                        let resample = resample::add(&mut dag, settings).unwrap();
                        dag.connect(output, resample.image).unwrap();
                        output = resample.output;
                    }
                    let result =
                        evaluator.evaluate::<Rgb>(&dag, &Default::default(), output).unwrap();
                    // The identity copies nothing. Every other orientation is one launch.
                    assert_eq!(result.statistics.fused, usize::from(index != 0), "{orientation:?}");
                    let actual = result.data.host().unwrap();
                    assert_eq!(actual.len(), expected[index].len());
                    for (actual, &source) in actual.iter().zip(expected[index]) {
                        for (actual, expected) in actual.iter().zip(samples[source]) {
                            assert_eq!(actual.to_bits(), expected.to_bits(), "{orientation:?}");
                        }
                    }
                }
            }
        }
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn cpu_permutations() {
        permutations(RuntimeContext::cpu());
    }

    #[test]
    #[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
    fn selected_backend_permutations() {
        permutations(RuntimeContext::from_env().unwrap());
    }
}
