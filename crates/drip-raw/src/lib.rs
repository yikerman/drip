//! Bundled RAW decoding through LibRaw, libjpeg-turbo and zlib.
//! The entry point, [`decode`], opens a file,
//! copies everything Drip needs into owned values and closes LibRaw before
//! returning, so no LibRaw state outlives a call or is shared between threads.
//! Linking the reentrant `libraw_r` makes concurrent calls safe.
//! [`Metadata`] derives serde, so callers can store decoded facts. Lens
//! correction tables the camera embedded come back raw in
//! [`Lens::correction`]: drip-raw decodes faithfully and leaves their
//! interpretation to the caller.

use std::ffi::{CStr, CString, c_char, c_int, c_longlong, c_uint, c_void};
use std::fmt;
use std::path::Path;

const CBLACK_SIZE: usize = 4104;

/// What [`decode`] returns for a given file, as a number. Bump it with every
/// change to the bundled LibRaw, its build options or the shim that can change
/// decoded samples or metadata: Drip keys stored decodes by it.
pub const VERSION: u32 = 2;

/// Mirrors `drip_raw_info` in `shim/shim.c`.
#[repr(C)]
struct Info {
    width: c_int,
    height: c_int,
    cfa: [[c_int; 2]; 2],
    flip: c_int,
    black: c_uint,
    maximum: c_uint,
    cblack: [c_uint; CBLACK_SIZE],
    cam_xyz: [[f32; 3]; 4],
    cam_mul: [f32; 4],
    make: [c_char; 64],
    model: [c_char; 64],
    iso_speed: f32,
    shutter: f32,
    aperture: f32,
    focal_len: f32,
    timestamp: c_longlong,
    datetime: [c_char; 20],
    raw_width: u32,
    raw_height: u32,
    left_margin: u32,
    top_margin: u32,
    pixel_aspect: f64,
    artist: [c_char; 64],
    description: [c_char; 512],
    shot_order: u32,
    body_serial: [c_char; 64],
    internal_body_serial: [c_char; 64],
    lens_make: [c_char; 128],
    lens_model: [c_char; 128],
    lens_serial: [c_char; 128],
    lens_internal_serial: [c_char; 128],
    lens_id: u64,
    lens_maker_note_model: [c_char; 128],
    lens_min_focal: f32,
    lens_max_focal: f32,
    lens_max_aperture_wide: f32,
    lens_max_aperture_tele: f32,
    focal_length_35mm: f32,
    min_focus_distance: f32,
    lens_mount: u32,
    camera_mount: u32,
    lens_format: u32,
    camera_format: u32,
    teleconverter: [c_char; 128],
    teleconverter_id: u64,
    adapter: [c_char; 128],
    adapter_id: u64,
    attachment: [c_char; 128],
    attachment_id: u64,
    latitude: [f32; 3],
    longitude: [f32; 3],
    gps_timestamp: [f32; 3],
    altitude: f32,
    latitude_ref: u8,
    longitude_ref: u8,
    altitude_ref: u8,
    gps_status: u8,
    gps_parsed: u8,
    pre_mul: [f32; 4],
    analog_balance: [f32; 4],
    linear_max: [u32; 4],
    rgb_cam: [[f32; 4]; 3],
    camera_matrix: [[f32; 4]; 3],
    dng_white_level: [u32; 4],
    dng_as_shot_neutral: [f32; 4],
    dng_user_crop: [f32; 4],
    baseline_exposure: f32,
    inset_crops: [[u32; 4]; 2],
    masked_areas: [[i32; 4]; 8],
    shooting_modes: [i32; 7],
    dng_default_crop: [u32; 4],
    dng_color: [DngColor; 2],
}

unsafe extern "C" {
    fn drip_raw_open(
        path: *const c_char,
        ifd_entry: lens::EntryCallback,
        note_entry: lens::EntryCallback,
        entries: *mut c_void,
        handle: *mut *mut c_void,
        info: *mut Info,
    ) -> c_int;
    fn drip_raw_copy(handle: *mut c_void, dst: *mut u16);
    fn drip_raw_close(handle: *mut c_void);
    fn drip_raw_strerror(err: c_int) -> *const c_char;
    fn drip_raw_unsupported(err: c_int) -> c_int;
    #[cfg(feature = "reference")]
    fn drip_raw_reference(
        path: *const c_char,
        width: *mut c_int,
        height: *mut c_int,
        rgb: *mut *mut u16,
    ) -> c_int;
    #[cfg(feature = "reference")]
    fn drip_free(p: *mut c_void);
}

mod lens;
mod metadata;
pub use lens::{Correction, DngVignette, DngWarp};
pub use metadata::*;

/// Visible Bayer samples in sensor order and their owned capture metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub data: Vec<u16>,
    pub metadata: Metadata,
}

// LibRaw 0.22.2 metadata/tiff.cpp maps EXIF orientation to transpose/flip bits.
// Invert that convention here; unknown orientation leaves the sensor order intact.
fn exif_orientation(flip: c_int) -> u8 {
    usize::try_from(flip).ok().and_then(|i| [1, 2, 4, 3, 5, 8, 6, 7].get(i).copied()).unwrap_or(1)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    message: String,
    unsupported: bool,
}

impl Error {
    /// LibRaw lacks this file format or decoder. Drip's Bayer-only restriction,
    /// other decoder errors and I/O errors are deliberately excluded.
    pub fn is_unsupported(&self) -> bool {
        self.unsupported
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

fn error(code: c_int) -> Error {
    if code > 0 {
        // LibRaw passes system errors through as positive errno values.
        return Error {
            message: std::io::Error::from_raw_os_error(code).to_string(),
            unsupported: false,
        };
    }
    // SAFETY: returns a pointer to a static NUL-terminated string.
    let message = unsafe { CStr::from_ptr(drip_raw_strerror(code)) }.to_string_lossy().into_owned();
    // SAFETY: a pure classification of LibRaw's integer error codes.
    let unsupported = unsafe { drip_raw_unsupported(code) != 0 };
    Error { message, unsupported }
}

fn c_path(path: &Path) -> Result<CString, Error> {
    // LibRaw takes narrow paths; on Windows that limits paths to ANSI (agent-docs/TODO.md).
    let path = path.to_str().ok_or_else(|| Error {
        message: format!("{} is not valid UTF-8", path.display()),
        unsupported: false,
    })?;
    CString::new(path)
        .map_err(|_| Error { message: "path contains NUL".into(), unsupported: false })
}

/// Closes the LibRaw handle on every exit path.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful drip_raw_open and is closed once.
        unsafe { drip_raw_close(self.0) }
    }
}

/// Decodes the Bayer raw at `path`. Other sensor layouts are rejected.
pub fn decode(path: &Path) -> Result<Raw, Error> {
    let path = c_path(path)?;
    // SAFETY: Info is plain integers and floats, for which all-zero is valid.
    let mut info: Box<Info> = Box::new(unsafe { std::mem::zeroed() });
    let mut handle = std::ptr::null_mut();
    let mut entries = lens::Entries::default();
    // SAFETY: valid NUL-terminated path and out-pointers, and callbacks whose
    // context is `entries`, which outlives the call; on success the shim
    // hands over a handle that only `Handle` closes.
    let code = unsafe {
        drip_raw_open(
            path.as_ptr(),
            lens::ifd_entry,
            lens::note_entry,
            (&raw mut entries).cast(),
            &mut handle,
            &mut *info,
        )
    };
    if code != 0 {
        return Err(error(code));
    }
    let handle = Handle(handle);
    let (width, height) = (info.width as usize, info.height as usize);
    let mut data = vec![0; width * height];
    // SAFETY: the shim writes exactly width * height values, which `data` holds;
    // it checked that the visible area lies inside the raw buffer.
    unsafe { drip_raw_copy(handle.0, data.as_mut_ptr()) };
    drop(handle);

    let c = &info.cblack;
    let (rows, cols) = (c[4] as usize, c[5] as usize);
    let text = |s: &[c_char]| {
        // SAFETY: the shim NUL-terminates its strings within their arrays.
        unsafe { CStr::from_ptr(s.as_ptr()) }.to_string_lossy().trim().to_owned()
    };
    Ok(Raw {
        data,
        metadata: Metadata {
            make: text(&info.make),
            model: text(&info.model),
            iso: info.iso_speed,
            shutter: info.shutter,
            aperture: info.aperture,
            focal_length: info.focal_len,
            timestamp: info.timestamp,
            datetime: text(&info.datetime),
            orientation: exif_orientation(info.flip),
            artist: text(&info.artist),
            description: text(&info.description),
            shot_order: info.shot_order,
            body_serial: text(&info.body_serial),
            internal_body_serial: text(&info.internal_body_serial),
            shooting_modes: info.shooting_modes,
            sensor: Sensor {
                width,
                height,
                raw_width: info.raw_width,
                raw_height: info.raw_height,
                left_margin: info.left_margin,
                top_margin: info.top_margin,
                pixel_aspect: info.pixel_aspect,
                inset_crops: info.inset_crops,
                masked_areas: info.masked_areas,
            },
            calibration: Calibration {
                cfa: info.cfa.map(|row| row.map(|color| color as u8)),
                black: info.black,
                channel_black: [c[0], c[1], c[2], c[3]],
                pattern: BlackPattern {
                    height: rows,
                    width: cols,
                    values: c[6..6 + rows * cols].to_vec(),
                },
                maximum: info.maximum,
                linear_max: info.linear_max,
                xyz_to_cam: info.cam_xyz,
                as_shot: info.cam_mul,
                reference_gains: info.pre_mul,
                analog_balance: info.analog_balance,
                rgb_cam: info.rgb_cam,
                camera_matrix: info.camera_matrix,
                dng_color: info.dng_color,
                dng_white_level: info.dng_white_level,
                dng_as_shot_neutral: info.dng_as_shot_neutral,
                dng_default_crop: info.dng_default_crop,
                dng_user_crop: info.dng_user_crop,
                baseline_exposure: info.baseline_exposure,
            },
            lens: Lens {
                make: text(&info.lens_make),
                model: text(&info.lens_model),
                serial: text(&info.lens_serial),
                internal_serial: text(&info.lens_internal_serial),
                id: info.lens_id,
                maker_note_model: text(&info.lens_maker_note_model),
                min_focal: info.lens_min_focal,
                max_focal: info.lens_max_focal,
                max_aperture_at_min_focal: info.lens_max_aperture_wide,
                max_aperture_at_max_focal: info.lens_max_aperture_tele,
                focal_length_35mm: info.focal_length_35mm,
                min_focus_distance: info.min_focus_distance,
                lens_mount: info.lens_mount,
                camera_mount: info.camera_mount,
                lens_format: info.lens_format,
                camera_format: info.camera_format,
                teleconverter: text(&info.teleconverter),
                teleconverter_id: info.teleconverter_id,
                adapter: text(&info.adapter),
                adapter_id: info.adapter_id,
                attachment: text(&info.attachment),
                attachment_id: info.attachment_id,
                correction: entries.correction(),
            },
            gps: (info.gps_parsed != 0).then_some(Gps {
                latitude: info.latitude,
                longitude: info.longitude,
                timestamp: info.gps_timestamp,
                altitude: info.altitude,
                latitude_ref: info.latitude_ref,
                longitude_ref: info.longitude_ref,
                altitude_ref: info.altitude_ref,
                status: info.gps_status,
            }),
        },
    })
}

/// LibRaw's own processing of `path`: half size (2×2 binning, greens
/// averaged), as-shot white balance, clipped highlights, linear Rec.2020,
/// 16 bit, unrotated. Returns width, height and row-major pixels.
#[cfg(feature = "reference")]
pub fn reference(path: &Path) -> Result<(usize, usize, Vec<[u16; 3]>), Error> {
    let path = c_path(path)?;
    let (mut width, mut height, mut rgb) = (0, 0, std::ptr::null_mut());
    // SAFETY: valid path and out-pointers; on success rgb is a malloc'd buffer
    // of width * height * 3 values that we copy and free exactly once.
    unsafe {
        let code = drip_raw_reference(path.as_ptr(), &mut width, &mut height, &mut rgb);
        if code != 0 {
            return Err(error(code));
        }
        let (width, height) = (width as usize, height as usize);
        let pixels = std::slice::from_raw_parts(rgb.cast::<[u16; 3]>(), width * height).to_vec();
        drip_free(rgb.cast());
        Ok((width, height, pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libraw_orientation_maps_to_all_exif_transforms() {
        for (exif, flip) in [(1, 0), (2, 1), (3, 3), (4, 2), (5, 4), (6, 6), (7, 7), (8, 5)] {
            assert_eq!(exif_orientation(flip), exif);
        }
        for unknown in [-1, 8, 90] {
            assert_eq!(exif_orientation(unknown), 1);
        }
        assert_eq!(Metadata::default().orientation, 1);
    }
}
