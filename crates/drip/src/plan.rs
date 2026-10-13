//! The segment compiler: consecutive maps run as one kernel.
//!
//! A map node publishes a deferred image instead of launching a kernel. Its
//! [`Plan`] lists the map bodies between the last raster and the image, with
//! their constants. Materializing the image runs the fused kernel once: it
//! loads each pixel, applies every body in order, and stores the result, so
//! intermediate images never reach memory. CubeCL compiles one kernel per
//! distinct body sequence and reuses it for any constant values.
use crate::{
    Result,
    image::Raster,
    runtime::{KernelContext, dispatch_dims},
    shape::{MapBody, MapPayload, Pixel},
};
use cubecl::prelude::*;
use std::sync::Arc;

/// Map bodies to apply in order, with their constants concatenated.
#[derive(Clone, Default)]
pub(crate) struct Plan {
    bodies: Vec<MapBody>,
    constants: Vec<f32>,
}
impl Plan {
    /// This plan followed by `body`. `constants` holds `body.constants` values.
    pub fn then(&self, body: &MapBody, constants: &[f32]) -> Self {
        let mut plan = self.clone();
        plan.bodies.push(*body);
        plan.constants.extend_from_slice(constants);
        plan
    }

    /// Run the plan on `input` into a new device raster described by `desc`,
    /// which holds as many pixels as the input.
    pub fn run<P: MapPayload>(
        &self,
        input: &Raster<P>,
        desc: Arc<P::Desc>,
        ctx: &KernelContext<'_>,
    ) -> Result<Raster<P>> {
        let output = Raster::from_device(desc.clone(), ctx.device_buffer::<P>(&desc)?, ctx)?;
        let constants = ctx.constants(&self.constants)?;
        let (count, dim) = dispatch_dims(P::byte_len(&desc) / size_of::<[f32; 3]>());
        kernel::fused::launch(
            ctx.client()?,
            count,
            dim,
            input.device(ctx)?.argument(),
            constants.argument(),
            output.device(ctx)?.argument(),
            Bodies(self.bodies.clone()),
        );
        ctx.runtime.count_fused();
        Ok(output)
    }
}

/// The body sequence of a fused kernel, part of its CubeCL kernel identity.
/// Constant windows follow from the bodies' constant counts, so equal
/// sequences compile to the same kernel whatever the constant values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Bodies(Vec<MapBody>);

mod kernel {
    use super::*;

    /// Apply each body to `px` with its window of `k`. The loop runs while
    /// the kernel is expanded, so the compiled kernel holds the bodies inline.
    #[cube]
    fn apply(px: Pixel, k: &[f32], #[comptime] bodies: &Bodies) -> Pixel {
        intrinsic!(|scope| {
            let mut px = px;
            let mut offset = 0usize;
            for body in &bodies.0 {
                let end = offset + body.constants;
                let window = k.__expand_slice_method(scope, offset.into(), end.into());
                px = (body.expand)(scope, px, window);
                offset = end;
            }
            px
        })
    }

    #[cube(launch)]
    pub fn fused(input: &[f32], k: &[f32], output: &mut [f32], #[comptime] bodies: Bodies) {
        let i = ABSOLUTE_POS * 3;
        if i < input.len() {
            let px = Pixel { r: input[i], g: input[i + 1], b: input[i + 2] };
            let px = apply(px, k, &bodies);
            output[i] = px.r;
            output[i + 1] = px.g;
            output[i + 2] = px.b;
        }
    }
}
