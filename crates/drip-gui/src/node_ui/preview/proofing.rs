//! ICC proofing for the preview working-space image.

use drip::node::profile;
use lcms2::{Flags, Intent, PixelFormat, Profile, ThreadContext, Transform};

pub(super) fn softproof(
    output: &profile::Output,
    pixels: &[[f32; 3]],
) -> Result<Vec<[f32; 3]>, String> {
    let mut proof: Vec<[f32; 3]> = output.convert(PixelFormat::RGB_FLT, pixels)?;
    // Interpret the mapped device values; intent and BPC belong to the forward
    // conversion, not to this return to the display pipeline's working space.
    let transform = Transform::new_flags_context(
        lcms2::GlobalContext::new(),
        &output.profile,
        PixelFormat::RGB_FLT,
        &profile::rec2020_linear(),
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_CACHE,
    )
    .map_err(|e| e.to_string())?;
    profile::convert_in_place(&mut proof, |chunk| {
        // A float proofing transform can cancel a matrix profile's round trip,
        // preserving colors outside its gamut. Bound device RGB explicitly, as
        // integer export does, without quantizing the preview's intermediate values.
        for pixel in &mut *chunk {
            *pixel = pixel.map(|v| v.clamp(0.0, 1.0));
        }
        transform.transform_in_place(chunk);
    });
    Ok(proof)
}

pub(super) fn gamutcheck(
    output: &profile::Output,
    pixels: &[[f32; 3]],
) -> Result<Vec<[f32; 3]>, String> {
    // Keep alarm settings private to this evaluation: other previews and export
    // can run concurrently. Profiles are reopened in the same LCMS context.
    let mut context = ThreadContext::new();
    let mut alarm = [0; 16];
    alarm[1] = u16::MAX;
    alarm[2] = u16::MAX;
    context.set_alarm_codes(alarm);
    let icc = profile::rec2020_linear().icc().map_err(|e| e.to_string())?;
    let working = Profile::new_icc_context(&context, &icc).map_err(|e| e.to_string())?;
    let target = Profile::new_icc_context(&context, &output.icc).map_err(|e| e.to_string())?;
    // Sample the gamut in perceptual coordinates, as darktable's colorout does
    // [14]. A uniform linear-RGB grid is too coarse near black.
    let lab =
        Profile::new_lab4_context(&context, lcms2::CIExyY::d50()).map_err(|e| e.to_string())?;
    let to_lab = Transform::new_flags_context(
        &context,
        &working,
        PixelFormat::RGB_FLT,
        &lab,
        PixelFormat::Lab_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_CACHE,
    )
    .map_err(|e| e.to_string())?;
    let transform = Transform::new_proofing_context(
        &context,
        &lab,
        PixelFormat::Lab_FLT,
        &working,
        PixelFormat::RGB_FLT,
        &target,
        output.intent,
        Intent::RelativeColorimetric,
        output.flags | Flags::SOFT_PROOFING | Flags::GAMUT_CHECK | Flags::NO_CACHE,
    )
    .map_err(|e| e.to_string())?;
    Ok(profile::convert_pixels(pixels, |input, output: &mut [[f32; 3]]| {
        to_lab.transform_pixels(input, output);
        transform.transform_in_place(output);
        for pixel in output {
            // Older LCMS versions use -1 in float output instead of alarm codes.
            if *pixel == [-1.0; 3] {
                *pixel = [0.0, 1.0, 1.0];
            }
        }
    }))
}
