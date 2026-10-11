use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, crate::Choice)]
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
    fn resolve(self, metadata: Option<&MetadataDesc>) -> u8 {
        match self {
            Self::FromMetadata => metadata.map_or(1, |d| d.orientation),
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

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Use capture orientation, or replace it with an explicit transform.
    #[label("Orientation")]
    #[param(Orientation::FromMetadata.schema())]
    pub orientation: Orientation,
}

fn orientation_contract(
    _: &GlobalContext,
    p: &Settings,
    image: Option<&ImageDesc<Color>>,
    metadata: Option<&MetadataDesc>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    let mut output = image.cloned();
    if p.orientation.resolve(metadata) >= 5
        && let Some(d) = &mut output
    {
        std::mem::swap(&mut d.extent.width, &mut d.extent.height);
    }
    Ok((output,))
}

/// Rotate or reflect ColorRgb pixels using CaptureMetadata orientation.
/// Missing metadata means identity; an explicit orientation replaces metadata.
/// Transpose reflects across the top-left to bottom-right diagonal; transverse
/// reflects across the other diagonal. Quarter turns and diagonal reflections
/// exchange width and height. Samples and color interpretation are preserved,
/// without resampling or clipping. Apply once after demosaic and color conversion.
#[crate::node(id="geometry.orientation", name="Orientation", category="Geometry", contract=orientation_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<ColorRgb>>,
    metadata: Option<Read<'_, Cpu<CaptureMetadata>>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    let orientation = p.orientation.resolve(metadata.as_ref().map(|m| m.desc));
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    kernel::orient::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        output.data.argument(),
        image.desc.extent.width as usize,
        image.desc.extent.height as usize,
        orientation as usize,
    );
    Ok(())
}

mod kernel {
    use cubecl::prelude::*;

    #[cube(launch)]
    pub fn orient(
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
        orientation: usize,
    ) {
        let i = ABSOLUTE_POS;
        if i < width * height {
            let mut x = i % width;
            let mut y = i / width;
            if orientation == 2 || orientation == 3 || orientation == 7 || orientation == 8 {
                x = width - 1 - x;
            }
            if orientation == 3 || orientation == 4 || orientation == 6 || orientation == 7 {
                y = height - 1 - y;
            }
            let mut target = y * width + x;
            if orientation >= 5 {
                target = x * height + y;
            }
            for c in 0..3 {
                output[target * 3 + c] = input[i * 3 + c];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeContext;

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

    fn image_desc() -> ImageDesc<Color> {
        ImageDesc {
            extent: Extent { width: 3, height: 2 },
            interpretation: Color {
                space: "Rec.2020".into(),
                encoding: Encoding::Srgb,
                scale: "display".into(),
            },
        }
    }

    #[test]
    fn metadata_defaults_and_explicit_override_preserve_interpretation() {
        let global = GlobalContext::default();
        let image = image_desc();
        let automatic = Settings::default();
        let (output,) = orientation_contract(&global, &automatic, Some(&image), None).unwrap();
        assert_eq!(output.unwrap().extent, image.extent);
        for (i, orientation) in EXPLICIT.into_iter().enumerate() {
            let metadata = MetadataDesc::of(&CaptureData {
                capture: drip_raw::Metadata { orientation: i as u8 + 1, ..Default::default() },
                ..Default::default()
            });
            let (output,) =
                orientation_contract(&global, &automatic, Some(&image), Some(&metadata)).unwrap();
            let output = output.unwrap();
            let expected =
                if i < 4 { image.extent.clone() } else { Extent { width: 2, height: 3 } };
            assert_eq!(output.extent, expected);
            assert_eq!(output.interpretation, image.interpretation);
            assert_eq!(orientation.resolve(None), metadata.orientation);
            let identity = Settings { orientation: Orientation::Identity };
            let (output,) =
                orientation_contract(&global, &identity, Some(&image), Some(&metadata)).unwrap();
            assert_eq!(output.unwrap().extent, image.extent);
            assert_eq!(identity.orientation.resolve(Some(&metadata)), 1);
            assert!(
                orientation_contract(&global, &automatic, None, Some(&metadata))
                    .unwrap()
                    .0
                    .is_none()
            );
        }
    }

    fn permutations(runtime: RuntimeContext) {
        // Row-major source: [0 1 2; 3 4 5]. Explicit destination orders make
        // clockwise/counterclockwise and both diagonal reflections distinct.
        fixture(
            &runtime,
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
            &runtime,
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
            &runtime,
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
        fixture(&runtime, 1, 1, [&[0]; 8]);
    }

    fn fixture(runtime: &RuntimeContext, width: u32, height: u32, expected: [&[usize]; 8]) {
        let image = ImageDesc { extent: Extent { width, height }, ..image_desc() };
        let samples: Vec<[f32; 3]> =
            (0..image.extent.pixels()).map(|i| [i as f32, -(i as f32), 8.0]).collect();
        let input = ColorRgb::upload(&image, &samples.clone().into(), runtime).unwrap();
        let ctx = KernelContext { global: &GlobalContext::default(), runtime };
        for (index, orientation) in EXPLICIT.into_iter().enumerate() {
            // Exercise both metadata resolution and replacement of a conflicting
            // metadata orientation, using the same exact pixel expectations.
            for mode in 0..if index == 0 { 3 } else { 2 } {
                let p = Settings {
                    orientation: if mode == 1 { orientation } else { Orientation::FromMetadata },
                };
                let data = CaptureData {
                    capture: drip_raw::Metadata {
                        orientation: if mode == 0 { index as u8 + 1 } else { 1 },
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let metadata = MetadataDesc::of(&data);
                let metadata = (mode != 2).then_some(&metadata);
                let (output_desc,) =
                    orientation_contract(ctx.global, &p, Some(&image), metadata).unwrap();
                let output_desc = output_desc.unwrap();
                let mut output =
                    <Device<ColorRgb> as Port>::allocate(&output_desc, runtime).unwrap();
                compute(
                    &ctx,
                    &p,
                    Read { desc: &image, data: &input },
                    metadata.map(|desc| Read { desc, data: &data }),
                    Write { desc: &output_desc, data: &mut output },
                )
                .unwrap();
                let actual = ColorRgb::download(&output_desc, &output, runtime).unwrap();
                assert_eq!(actual.len(), expected[index].len());
                for (actual, &source) in actual.iter().zip(expected[index]) {
                    for (actual, expected) in actual.iter().zip(samples[source]) {
                        assert_eq!(actual.to_bits(), expected.to_bits(), "{orientation:?}");
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
