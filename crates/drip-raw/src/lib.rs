//! Bundled RAW decoding through LibRaw, libjpeg-turbo and zlib.
//! The entry point, [`decode`], opens a file,
//! copies everything Drip needs into owned values and closes LibRaw before
//! returning, so no LibRaw state outlives a call or is shared between threads.
//! Linking the reentrant `libraw_r` makes concurrent calls safe.

use std::ffi::{CStr, CString, c_char, c_int, c_longlong, c_uint, c_void};
use std::fmt;
use std::path::Path;

const CBLACK_SIZE: usize = 4104;

/// Mirrors `drip_raw_info` in `shim/shim.c`.
#[repr(C)]
struct Info {
    width: c_int,
    height: c_int,
    cfa: [[c_int; 2]; 2],
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
}

unsafe extern "C" {
    fn drip_raw_open(path: *const c_char, handle: *mut *mut c_void, info: *mut Info) -> c_int;
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

/// The visible area of a Bayer raw as stored by the camera, before any
/// processing, with what is needed to interpret it.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub width: usize,
    pub height: usize,
    /// Row-major sensor values.
    pub data: Vec<u16>,
    /// Color index (0 R, 1 G, 2 B, 3 second G) at `[row % 2][col % 2]`.
    pub cfa: [[u8; 2]; 2],
    /// The black level at (row, col) is
    /// `black + channel_black[cfa color] + pattern.at(row, col)`.
    pub black: u32,
    pub channel_black: [u32; 4],
    pub pattern: BlackPattern,
    /// Sensor saturation level.
    pub maximum: u32,
    /// Maps CIE XYZ (D65) to camera RGB; LibRaw calls it `cam_xyz`.
    pub xyz_to_cam: [[f32; 3]; 3],
    /// As-shot white balance multipliers per color index; any may be 0 if unknown.
    pub as_shot: [f32; 4],
    pub metadata: Metadata,
}

/// A black level offset repeating over `height` × `width` sites; empty if none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlackPattern {
    pub height: usize,
    pub width: usize,
    pub values: Vec<u32>,
}

impl BlackPattern {
    pub fn at(&self, row: usize, col: usize) -> u32 {
        if self.values.is_empty() {
            0
        } else {
            self.values[row % self.height * self.width + col % self.width]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub iso: f32,
    /// Seconds.
    pub shutter: f32,
    pub aperture: f32,
    /// Millimeters.
    pub focal_length: f32,
    /// Unix time.
    pub timestamp: i64,
    /// Capture time as the camera's EXIF `YYYY:MM:DD HH:MM:SS`; empty if unknown.
    pub datetime: String,
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
    // SAFETY: valid NUL-terminated path and out-pointers; on success the shim
    // hands over a handle that only `Handle` closes.
    let code = unsafe { drip_raw_open(path.as_ptr(), &mut handle, &mut *info) };
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
        width,
        height,
        data,
        cfa: info.cfa.map(|row| row.map(|color| color as u8)),
        black: info.black,
        channel_black: [c[0], c[1], c[2], c[3]],
        pattern: BlackPattern { height: rows, width: cols, values: c[6..6 + rows * cols].to_vec() },
        maximum: info.maximum,
        xyz_to_cam: [info.cam_xyz[0], info.cam_xyz[1], info.cam_xyz[2]],
        as_shot: info.cam_mul,
        metadata: Metadata {
            make: text(&info.make),
            model: text(&info.model),
            iso: info.iso_speed,
            shutter: info.shutter,
            aperture: info.aperture,
            focal_length: info.focal_len,
            timestamp: info.timestamp,
            datetime: text(&info.datetime),
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
