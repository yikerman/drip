// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2009-2026 darktable developers.
//! Lens corrections the camera embeds in the file, read as darktable reads
//! them \[1\]. LibRaw parses these tags but keeps none of them, so
//! [`decode`](crate::decode) collects the entries LibRaw reports to its
//! metadata callbacks in `Entries`, which picks and validates them by
//! darktable's rules. Values stay as the file stores them: Drip's lens
//! correction node interprets them.
//!
//! Deviations, all on malformed or unusual files: an unexpected TIFF type or
//! length, a zero denominator, a non-finite value, a malformed opcode list
//! or a one-knot Sony table yields no correction, where darktable may read
//! partial or undefined values. Fujifilm tables come from the RAF's raw IFD
//! only (see `Entries::fuji`).
//!
//! \[1\] darktable developers, “exif.cc” (`_check_lens_correction_data`),
//! “dng_opcode.c” and “image.h” (`dt_image_correction_data_t`), commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3, 2026. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/common>
//! License: GPL-3.0-or-later; see THIRD_PARTY.md for credits and the upstream license.

use std::ffi::{c_int, c_longlong, c_uint, c_void};

unsafe extern "C" {
    /// Reads up to `len` bytes from the LibRaw stream a metadata callback
    /// received and returns how many it read (`shim/stream.cpp`).
    fn drip_raw_read(stream: *mut c_void, dst: *mut u8, len: usize) -> usize;
}

/// One vendor's embedded correction, laid out as darktable's
/// `dt_image_correction_data_t` keeps it. Spline tables hold one value per
/// knot. darktable's `_init_coeffs_md_v2` gives the values their meaning.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Correction {
    /// Sony `DistortionCorrParams` (0x7037), `ChromaticAberrationCorrParams`
    /// (0x7035) and `VignettingCorrParams` (0x7032), without the leading knot
    /// count. 2 to 16 knots.
    Sony { distortion: Vec<i16>, ca_red: Vec<i16>, ca_blue: Vec<i16>, vignetting: Vec<i16> },
    /// Fujifilm `GeometricDistortionParams` (0xf00b),
    /// `ChromaticAberrationParams` (0xf00f) and `VignettingParams` (0xf010)
    /// of the RAF's raw IFD, rationals as `f32`. 9 knots, or 11 on X-Trans
    /// I–III era bodies, whose first knot carries no CA and reads as zero.
    /// `crop_mode` is the maker note's `CropMode` (0x104d).
    Fuji {
        knots: Vec<f32>,
        distortion: Vec<f32>,
        ca_red: Vec<f32>,
        ca_blue: Vec<f32>,
        vignetting: Vec<f32>,
        crop_mode: Option<u16>,
    },
    /// DNG `OpcodeList3` of the raw IFD: the last `WarpRectilinear` and
    /// `FixVignetteRadial`, at least one present.
    Dng { warp: Option<DngWarp>, vignette: Option<DngVignette> },
    /// Olympus ImageProcessing 0x150a (distortion) and 0x150c (CA), each
    /// kept only if it corrects something: `0 0 0 1` is the null distortion
    /// and all zeros the null CA.
    Olympus { distortion: Option<[f32; 4]>, ca: Option<[f32; 6]> },
    /// Panasonic `DistortionInfo` (RW2 IFD0 0x0119) when its correction
    /// flag is on. `a`, `b`, `c` and `scale` are its words 8, 4, 11 and 5,
    /// signed Q15 fixed point.
    Panasonic { a: i16, b: i16, c: i16, scale: i16 },
}

/// DNG `WarpRectilinear` (opcode 1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DngWarp {
    /// `kr0..kr3, kt0, kt1` for one plane, or for each of three.
    pub planes: Vec<[f64; 6]>,
    /// Optical centre relative to the image, `[x, y]` in [0, 1].
    pub centre: [f64; 2],
}

/// DNG `FixVignetteRadial` (opcode 3).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DngVignette {
    /// `k0..k4`.
    pub k: [f64; 5],
    /// Optical centre relative to the image, `[x, y]` in [0, 1].
    pub centre: [f64; 2],
}

// Tag numbers as darktable names them through exiv2.
const SUB_IFDS: u32 = 0x014a;
const SONY_DISTORTION: u32 = 0x7037;
const SONY_CA: u32 = 0x7035;
const SONY_VIGNETTING: u32 = 0x7032;
const OPCODE_LIST3: u32 = 0xc74e;
const FUJI_DISTORTION: u32 = 0xf00b;
const FUJI_CA: u32 = 0xf00f;
const FUJI_VIGNETTING: u32 = 0xf010;
const FUJI_CROP_MODE: u32 = 0x104d;
// LibRaw reports a maker note sub-IFD entry as `tag | sub-IFD tag << 16`;
// 0x2040 is Olympus ImageProcessing (exiv2's `OlympusIp`).
const OLYMPUS_DISTORTION: u32 = 0x2040_150a;
const OLYMPUS_CA: u32 = 0x2040_150c;
const PANASONIC_DISTORTION: u32 = 0x0119;
// darktable's TIFF and DNG exports move Panasonic's tag here.
const PANASONIC_COPY: u32 = 0xf119;

/// Where LibRaw found an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    /// A TIFF IFD, numbered in parse order: IFD0 is 0.
    Ifd(u32),
    /// The first IFD of a Panasonic RW2, exiv2's `PanasonicRaw`.
    PanasonicRaw,
    MakerNote,
}

/// TIFF value types the vendors store these tags in.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Bytes(Vec<u8>),
    Short(Vec<u16>),
    SShort(Vec<i16>),
    Float(Vec<f32>),
    SRational(Vec<[i32; 2]>),
}

#[derive(Debug)]
struct Entry {
    dir: Dir,
    tag: u32,
    value: Value,
}

/// The correction entries LibRaw reported while it identified a file.
#[derive(Debug, Default)]
pub(crate) struct Entries {
    entries: Vec<Entry>,
    /// Highest IFD number reported so far.
    last_ifd: u32,
    /// `last_ifd` when IFD0 listed its SubIFDs.
    sub_ifds_listed: Option<u32>,
    /// The IFD exiv2 calls `SubImage1`, the first SubIFD of IFD0.
    sub_image1: Option<u32>,
}

/// LibRaw's `exif_parser_callback`: context, tag, TIFF type, value count,
/// byte order (`II` or `MM`), datastream and offset base.
pub(crate) type EntryCallback =
    unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_uint, *mut c_void, c_longlong);

/// LibRaw's `exif_parser_callback` for TIFF IFD entries. LibRaw marks the
/// tag with `(ifd + 1) << 20`, or with 0x30000 inside a Panasonic RW2 IFD
/// (`metadata/tiff.cpp`), and positions `stream` at the value.
///
/// # Safety
/// `entries` is the `Entries` passed to `drip_raw_open`, and `stream` is
/// LibRaw's datastream.
pub(crate) unsafe extern "C" fn ifd_entry(
    entries: *mut c_void,
    tag: c_int,
    ty: c_int,
    count: c_int,
    order: c_uint,
    stream: *mut c_void,
    _base: c_longlong,
) {
    let tag = tag as u32;
    let dir = match (tag >> 20, (tag >> 16) & 0xf) {
        (0, 3) => Dir::PanasonicRaw,
        (0, _) | (_, 1..) => return,
        (ifd, 0) => Dir::Ifd(ifd - 1),
    };
    // SAFETY: the caller's contract.
    unsafe { (*entries.cast::<Entries>()).record(dir, tag & 0xffff, ty, count, order, stream) }
}

/// LibRaw's `exif_parser_callback` for maker note entries
/// (`metadata/makernotes.cpp`).
///
/// # Safety
/// As [`ifd_entry`].
pub(crate) unsafe extern "C" fn note_entry(
    entries: *mut c_void,
    tag: c_int,
    ty: c_int,
    count: c_int,
    order: c_uint,
    stream: *mut c_void,
    _base: c_longlong,
) {
    // SAFETY: the caller's contract.
    unsafe {
        (*entries.cast::<Entries>()).record(Dir::MakerNote, tag as u32, ty, count, order, stream)
    }
}

impl Entries {
    /// Keeps the entry if a correction may live in it. Never panics: it runs
    /// inside a C callback.
    ///
    /// # Safety
    /// `stream` is the LibRaw datastream positioned at the entry's value.
    unsafe fn record(
        &mut self,
        dir: Dir,
        tag: u32,
        ty: c_int,
        count: c_int,
        order: c_uint,
        stream: *mut c_void,
    ) {
        if let Dir::Ifd(ifd) = dir {
            self.locate_sub_image1(ifd, tag);
        }
        let wanted = match dir {
            Dir::Ifd(_) => matches!(
                tag,
                SONY_DISTORTION
                    | SONY_CA
                    | SONY_VIGNETTING
                    | OPCODE_LIST3
                    | FUJI_DISTORTION
                    | FUJI_CA
                    | FUJI_VIGNETTING
                    | PANASONIC_COPY
            ),
            Dir::PanasonicRaw => tag == PANASONIC_DISTORTION,
            Dir::MakerNote => matches!(tag, FUJI_CROP_MODE | OLYMPUS_DISTORTION | OLYMPUS_CA),
        };
        // Far above any correction table, so a corrupt count cannot
        // allocate without bound.
        const MAX_BYTES: usize = 1 << 24;
        let Some((unit, _)) = unit(ty).filter(|_| wanted) else { return };
        let Some(len) = usize::try_from(count).ok().and_then(|n| n.checked_mul(unit)) else {
            return;
        };
        if len > MAX_BYTES {
            return;
        }
        let mut bytes = vec![0; len];
        // SAFETY: `bytes` holds `len` writable bytes, and `stream` is the
        // caller's LibRaw datastream.
        if unsafe { drip_raw_read(stream, bytes.as_mut_ptr(), len) } != len {
            return;
        }
        self.entries.push(Entry { dir, tag, value: Value::decode(ty, order == 0x4d4d, bytes) });
    }

    /// IFD0 lists its SubIFDs before LibRaw parses them, so `SubImage1` is
    /// the first IFD reported after that entry with a number above all
    /// earlier ones. An empty first SubIFD would shift this to the second,
    /// which only a malformed file has.
    fn locate_sub_image1(&mut self, ifd: u32, tag: u32) {
        match self.sub_ifds_listed {
            Some(last) if self.sub_image1.is_none() && ifd > last => self.sub_image1 = Some(ifd),
            None if ifd == 0 && tag == SUB_IFDS => self.sub_ifds_listed = Some(self.last_ifd),
            _ => {}
        }
        self.last_ifd = self.last_ifd.max(ifd);
    }

    fn find(&self, dir: Dir, tag: u32) -> Option<&Value> {
        self.entries.iter().find(|e| e.dir == dir && e.tag == tag).map(|e| &e.value)
    }

    /// `Exif.SubImage1.<tag>`.
    fn sub_image1(&self, tag: u32) -> Option<&Value> {
        self.find(Dir::Ifd(self.sub_image1?), tag)
    }

    /// The embedded correction, if one vendor's tags are complete and valid.
    /// darktable reads DNG opcodes, then Sony, Fuji, Olympus and Panasonic
    /// tags, and the last that parses wins. A file carries one vendor's.
    pub(crate) fn correction(&self) -> Option<Correction> {
        self.panasonic()
            .or_else(|| self.olympus())
            .or_else(|| self.fuji())
            .or_else(|| self.sony())
            .or_else(|| self.dng())
    }

    fn sony(&self) -> Option<Correction> {
        // SubImage1 if it holds all three tags, else IFD0, where darktable's
        // DNG exports put them.
        let tables = |ifd| {
            let [d, c, v] = [SONY_DISTORTION, SONY_CA, SONY_VIGNETTING]
                .map(|tag| self.find(Dir::Ifd(ifd), tag));
            Some([d?, c?, v?])
        };
        let [d, c, v] = self.sub_image1.and_then(tables).or_else(|| tables(0))?;
        let (Value::SShort(d), Value::SShort(c), Value::SShort(v)) = (d, c, v) else {
            return None;
        };
        // darktable's check, plus exact lengths and the two knots its knot
        // spacing `1 / (nc - 1)` needs.
        let nc = usize::try_from(*d.first()?).ok().filter(|nc| (2..=16).contains(nc))?;
        let valid = c.first() == Some(&(2 * nc as i16))
            && v.first() == Some(&(nc as i16))
            && (d.len(), c.len(), v.len()) == (nc + 1, 2 * nc + 1, nc + 1);
        valid.then(|| Correction::Sony {
            distortion: d[1..].to_vec(),
            ca_red: c[1..=nc].to_vec(),
            ca_blue: c[nc + 1..].to_vec(),
            vignetting: v[1..].to_vec(),
        })
    }

    fn fuji(&self) -> Option<Correction> {
        // exiv2 merges maker note and raw IFD tags into `Exif.Fujifilm`, maker
        // note first. Some bodies (X-A10) have an unrelated maker note blob
        // numbered 0xf010, which darktable then rejects, so read the raw IFD.
        let table = |tag| match self
            .entries
            .iter()
            .find(|e| matches!(e.dir, Dir::Ifd(_)) && e.tag == tag)
        {
            Some(Entry { value: Value::SRational(r), .. }) => r
                .iter()
                .map(|&[n, d]| (d != 0).then(|| n as f32 / d as f32))
                .collect::<Option<Vec<_>>>(),
            _ => None,
        };
        let (d, c, v) = (table(FUJI_DISTORTION)?, table(FUJI_CA)?, table(FUJI_VIGNETTING)?);
        // Each table leads with one value darktable ignores, then the knots.
        // CA holds red, then blue values.
        let (nc, c) = match (d.len(), c.len(), v.len()) {
            (19, 29, 19) => (9, c),
            // X-Trans I–III omit the first knot and its CA, both zero.
            (23, 31, 23) => {
                (11, [&c[..1], &[0.], &c[1..11], &[0.], &c[11..21], &[0.], &c[21..]].concat())
            }
            _ => return None,
        };
        let knots = &d[1..=nc];
        if knots != &c[1..=nc] || knots != &v[1..=nc] {
            return None;
        }
        let crop_mode = match self.find(Dir::MakerNote, FUJI_CROP_MODE) {
            Some(Value::Short(m)) if m.len() == 1 => Some(m[0]),
            _ => None,
        };
        Some(Correction::Fuji {
            knots: knots.to_vec(),
            distortion: d[nc + 1..=2 * nc].to_vec(),
            ca_red: c[nc + 1..=2 * nc].to_vec(),
            ca_blue: c[2 * nc + 1..=3 * nc].to_vec(),
            vignetting: v[nc + 1..=2 * nc].to_vec(),
            crop_mode,
        })
    }

    fn olympus(&self) -> Option<Correction> {
        let floats = |tag| match self.find(Dir::MakerNote, tag) {
            Some(Value::Float(f)) if f.iter().all(|k| k.is_finite()) => Some(f.as_slice()),
            _ => None,
        };
        let distortion = floats(OLYMPUS_DISTORTION)
            .and_then(|f| <[f32; 4]>::try_from(f).ok())
            .filter(|d| d[..3].iter().any(|&k| k != 0.));
        let ca = floats(OLYMPUS_CA)
            .and_then(|f| <[f32; 6]>::try_from(f).ok())
            .filter(|ca| ca.iter().any(|&k| k != 0.));
        (distortion.is_some() || ca.is_some()).then_some(Correction::Olympus { distortion, ca })
    }

    fn panasonic(&self) -> Option<Correction> {
        let blob = self
            .find(Dir::PanasonicRaw, PANASONIC_DISTORTION)
            .or_else(|| self.find(Dir::Ifd(0), PANASONIC_COPY));
        let Some(Value::Bytes(blob)) = blob else { return None };
        if blob.len() != 32 {
            return None;
        }
        // Sixteen little-endian words (darktable, after ExifTool's
        // PanasonicRaw.pm). Word 7's low nibble is 1 when correction is on.
        let w = |i: usize| i16::from_le_bytes([blob[2 * i], blob[2 * i + 1]]);
        (w(7) & 0xf == 1).then(|| Correction::Panasonic { a: w(8), b: w(4), c: w(11), scale: w(5) })
    }

    fn dng(&self) -> Option<Correction> {
        // darktable falls back to IFD0 for DNGs without a preview.
        let list = self.sub_image1(OPCODE_LIST3).or_else(|| self.find(Dir::Ifd(0), OPCODE_LIST3));
        let Some(Value::Bytes(list)) = list else { return None };
        dng_opcodes(list)
    }
}

/// Parses a DNG opcode list: a big-endian count, then per opcode its ID,
/// DNG version, flags, parameter size and parameters (DNG specification
/// 1.7.1, ch. 7).
fn dng_opcodes(list: &[u8]) -> Option<Correction> {
    fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        let (head, rest) = bytes.split_at_checked(n)?;
        *bytes = rest;
        Some(head)
    }
    fn u32(bytes: &mut &[u8]) -> Option<u32> {
        Some(u32::from_be_bytes(take(bytes, 4)?.try_into().ok()?))
    }
    fn f64s<const N: usize>(bytes: &mut &[u8]) -> Option<[f64; N]> {
        let values = take(bytes, 8 * N)?.as_chunks::<8>().0.iter().map(|&b| f64::from_be_bytes(b));
        values
            .collect::<Vec<_>>()
            .try_into()
            .ok()
            .filter(|v: &[f64; N]| v.iter().all(|x| x.is_finite()))
    }
    const WARP_RECTILINEAR: u32 = 1;
    const FIX_VIGNETTE_RADIAL: u32 = 3;
    let mut list = list;
    let (mut warp, mut vignette) = (None, None);
    for _ in 0..u32(&mut list)? {
        let [id, _version, _flags, size] = [(); 4].map(|_| u32(&mut list));
        let mut params = take(&mut list, usize::try_from(size?).ok()?)?;
        match id? {
            WARP_RECTILINEAR => {
                let planes = match u32(&mut params)? {
                    n @ (1 | 3) => (0..n).map(|_| f64s::<6>(&mut params)).collect::<Option<_>>()?,
                    _ => return None,
                };
                warp = Some(DngWarp { planes, centre: f64s(&mut params)? });
            }
            FIX_VIGNETTE_RADIAL => {
                vignette = Some(DngVignette { k: f64s(&mut params)?, centre: f64s(&mut params)? });
            }
            _ => continue,
        }
        if !params.is_empty() {
            return None;
        }
    }
    (warp.is_some() || vignette.is_some()).then_some(Correction::Dng { warp, vignette })
}

/// Bytes per value of a TIFF type, and per component a byte order swaps,
/// for the types these tags use.
fn unit(ty: c_int) -> Option<(usize, usize)> {
    match ty {
        Value::BYTE | Value::UNDEFINED => Some((1, 1)),
        Value::SHORT | Value::SSHORT => Some((2, 2)),
        Value::FLOAT => Some((4, 4)),
        Value::SRATIONAL => Some((8, 4)),
        _ => None,
    }
}

impl Value {
    const BYTE: c_int = 1;
    const SHORT: c_int = 3;
    const UNDEFINED: c_int = 7;
    const SSHORT: c_int = 8;
    const SRATIONAL: c_int = 10;
    const FLOAT: c_int = 11;

    /// Decodes `bytes` of a type [`unit`] accepts.
    fn decode(ty: c_int, big_endian: bool, mut bytes: Vec<u8>) -> Self {
        if let Some((_, component)) = unit(ty).filter(|_| big_endian) {
            bytes.chunks_exact_mut(component).for_each(<[u8]>::reverse);
        }
        let words = || bytes.as_chunks::<2>().0.iter();
        let dwords = || bytes.as_chunks::<4>().0.iter();
        match ty {
            Self::SHORT => Self::Short(words().map(|&b| u16::from_le_bytes(b)).collect()),
            Self::SSHORT => Self::SShort(words().map(|&b| i16::from_le_bytes(b)).collect()),
            Self::FLOAT => Self::Float(dwords().map(|&b| f32::from_le_bytes(b)).collect()),
            Self::SRATIONAL => Self::SRational(
                dwords().map(|&b| i32::from_le_bytes(b)).collect::<Vec<_>>().as_chunks().0.to_vec(),
            ),
            _ => Self::Bytes(bytes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(list: impl IntoIterator<Item = (Dir, u32, Value)>) -> Entries {
        Entries {
            entries: list.into_iter().map(|(dir, tag, value)| Entry { dir, tag, value }).collect(),
            sub_image1: Some(1),
            ..Default::default()
        }
    }

    fn sony(nc: i16, ca: i16, vignetting: i16) -> Entries {
        let table = |n: i16, len| Value::SShort([n].into_iter().chain((1..).take(len)).collect());
        entries([
            (Dir::Ifd(1), SONY_DISTORTION, table(nc, nc.max(0) as usize)),
            (Dir::Ifd(1), SONY_CA, table(ca, 2 * nc.max(0) as usize)),
            (Dir::Ifd(1), SONY_VIGNETTING, table(vignetting, nc.max(0) as usize)),
        ])
    }

    #[test]
    fn sony_tables_split_and_validate() {
        let Some(Correction::Sony { distortion, ca_red, ca_blue, vignetting }) =
            sony(3, 6, 3).correction()
        else {
            panic!("valid tables")
        };
        assert_eq!(
            (distortion, ca_red, ca_blue, vignetting.len()),
            (vec![1, 2, 3], vec![1, 2, 3], vec![4, 5, 6], 3)
        );
        for (nc, ca, vignetting) in [(3, 5, 3), (3, 6, 2), (17, 34, 17), (1, 2, 1), (-1, -2, -1)] {
            assert_eq!(sony(nc, ca, vignetting).correction(), None, "{nc} {ca} {vignetting}");
        }
        // A table of the wrong type or length is malformed.
        let mut e = sony(3, 6, 3);
        e.entries[0].value = Value::Short(vec![3, 1, 2, 3]);
        assert_eq!(e.correction(), None);
        let mut e = sony(3, 6, 3);
        e.entries[2].value = Value::SShort(vec![3, 1, 2]);
        assert_eq!(e.correction(), None);
    }

    #[test]
    fn sony_tables_come_from_sub_image1_else_ifd0() {
        let mut e = sony(3, 6, 3);
        e.entries.iter_mut().for_each(|e| e.dir = Dir::Ifd(2));
        assert_eq!(e.correction(), None, "another SubIFD is not SubImage1");
        e.entries.iter_mut().for_each(|e| e.dir = Dir::Ifd(0));
        assert!(e.correction().is_some());
    }

    #[test]
    fn sub_image1_is_the_first_ifd_after_ifd0_lists_sub_ifds() {
        let mut e = Entries::default();
        for (ifd, tag) in [(0, 0x0100), (0, SUB_IFDS), (1, 0x00fe), (2, 0x00fe), (0, 0x8769)] {
            e.locate_sub_image1(ifd, tag);
        }
        assert_eq!(e.sub_image1, Some(1));
        let mut e = Entries::default();
        for (ifd, tag) in [(0, 0x0100), (1, 0x00fe), (0, SUB_IFDS), (2, 0x00fe), (3, 0x00fe)] {
            e.locate_sub_image1(ifd, tag);
        }
        assert_eq!(e.sub_image1, Some(2), "IFD 1 was parsed before IFD0 listed SubIFDs");
    }

    fn rationals(values: &[f32]) -> Value {
        Value::SRational(values.iter().map(|&v| [(v * 64.) as i32, 64]).collect())
    }

    #[test]
    fn fuji_layouts_share_knots() {
        let knots = [0.25, 0.5, 0.75, 1., 1.25, 1.5, 1.75, 2., 2.25];
        let table = |values: &[f32]| [&[9.][..], &knots, values].concat();
        let fuji = |c: Vec<f32>| {
            entries([
                (Dir::Ifd(3), FUJI_DISTORTION, rationals(&table(&[1.; 9]))),
                (Dir::Ifd(3), FUJI_CA, rationals(&c)),
                (Dir::Ifd(3), FUJI_VIGNETTING, rationals(&table(&[50.; 9]))),
                (Dir::MakerNote, FUJI_CROP_MODE, Value::Short(vec![2])),
            ])
        };
        let ca = [table(&[2.; 9]), vec![3.; 9], vec![9.]].concat();
        let Some(Correction::Fuji { knots: k, distortion, ca_red, ca_blue, vignetting, crop_mode }) =
            fuji(ca.clone()).correction()
        else {
            panic!("X-Trans IV layout")
        };
        assert_eq!(
            (k, distortion, ca_red, ca_blue),
            (knots.to_vec(), vec![1.; 9], vec![2.; 9], vec![3.; 9])
        );
        assert_eq!((vignetting, crop_mode), (vec![50.; 9], Some(2)));
        let mut moved = ca;
        moved[2] = 0.375;
        assert_eq!(fuji(moved).correction(), None, "CA knots differ");

        // X-Trans I–III: 11 knots from 0, CA without the first.
        let knots: Vec<f32> = (0..11).map(|i| i as f32 / 8.).collect();
        let table = |v: f32| [&[11.][..], &knots, &[v; 11]].concat();
        let ca = [&[11.][..], &knots[1..], &[2.; 10], &[3.; 10]].concat();
        let e = entries([
            (Dir::Ifd(3), FUJI_DISTORTION, rationals(&table(1.))),
            (Dir::Ifd(3), FUJI_CA, rationals(&ca)),
            (Dir::Ifd(3), FUJI_VIGNETTING, rationals(&table(50.))),
        ]);
        let Some(Correction::Fuji { knots: k, ca_red, ca_blue, crop_mode, .. }) = e.correction()
        else {
            panic!("X-Trans I–III layout")
        };
        assert_eq!(k, knots);
        assert_eq!(ca_red, [&[0.][..], &[2.; 10]].concat());
        assert_eq!(ca_blue, [&[0.][..], &[3.; 10]].concat());
        assert_eq!(crop_mode, None);
    }

    #[test]
    fn fuji_rejects_maker_note_tables_and_zero_denominators() {
        let table = Value::SRational(vec![[1, 1]; 19]);
        let ca = Value::SRational(vec![[1, 1]; 29]);
        let fuji = |dir, ca: Value| {
            entries([
                (dir, FUJI_DISTORTION, table.clone()),
                (dir, FUJI_CA, ca),
                (dir, FUJI_VIGNETTING, table.clone()),
            ])
        };
        assert!(fuji(Dir::Ifd(5), ca.clone()).correction().is_some());
        assert_eq!(fuji(Dir::MakerNote, ca).correction(), None);
        let mut zero = vec![[1, 1]; 29];
        zero[12] = [1, 0];
        assert_eq!(fuji(Dir::Ifd(5), Value::SRational(zero)).correction(), None);
    }

    #[test]
    fn olympus_ignores_null_tables() {
        let olympus = |d: Vec<f32>, ca: Vec<f32>| {
            entries([
                (Dir::MakerNote, OLYMPUS_DISTORTION, Value::Float(d)),
                (Dir::MakerNote, OLYMPUS_CA, Value::Float(ca)),
            ])
            .correction()
        };
        assert_eq!(olympus(vec![0., 0., 0., 1.], vec![0.; 6]), None);
        assert_eq!(
            olympus(vec![0., 0., 0., 1.], vec![0., 1., 0., 0., 0., 0.]),
            Some(Correction::Olympus { distortion: None, ca: Some([0., 1., 0., 0., 0., 0.]) })
        );
        assert_eq!(
            olympus(vec![0.5, 0., 0., 1.], vec![0.; 5]),
            Some(Correction::Olympus { distortion: Some([0.5, 0., 0., 1.]), ca: None })
        );
        assert_eq!(olympus(vec![f32::NAN, 0., 0., 1.], vec![0.; 6]), None);
    }

    #[test]
    fn panasonic_needs_its_flag_and_32_bytes() {
        let blob = |flag: i16| -> Vec<u8> {
            let mut w = [0i16; 16];
            (w[4], w[5], w[7], w[8], w[11]) = (11001, -3, flag, 5932, -11502);
            w.iter().flat_map(|w| w.to_le_bytes()).collect()
        };
        let pana = |dir, tag, blob| entries([(dir, tag, Value::Bytes(blob))]).correction();
        let on = Some(Correction::Panasonic { a: 5932, b: 11001, c: -11502, scale: -3 });
        assert_eq!(pana(Dir::PanasonicRaw, PANASONIC_DISTORTION, blob(0x21)), on);
        assert_eq!(pana(Dir::Ifd(0), PANASONIC_COPY, blob(1)), on);
        assert_eq!(pana(Dir::PanasonicRaw, PANASONIC_DISTORTION, blob(0x10)), None);
        assert_eq!(pana(Dir::PanasonicRaw, PANASONIC_DISTORTION, blob(1)[..30].to_vec()), None);
    }

    fn opcode(id: u32, params: &[u8]) -> Vec<u8> {
        [id, 0x0103_0000, 1, params.len() as u32]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .chain(params.iter().copied())
            .collect()
    }

    fn doubles(values: &[f64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn dng_opcodes_parse_bounded() {
        let warp =
            [&1u32.to_be_bytes()[..], &doubles(&[0.98, 0.05, -0.05, 0.02, 0., 0., 0.47, 0.5])]
                .concat();
        let vignette = doubles(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.48, 0.51]);
        let list =
            |ops: &[Vec<u8>]| [&(ops.len() as u32).to_be_bytes()[..], &ops.concat()].concat();
        let all = list(&[opcode(9, &[0; 12]), opcode(1, &warp), opcode(3, &vignette)]);
        assert_eq!(
            dng_opcodes(&all),
            Some(Correction::Dng {
                warp: Some(DngWarp {
                    planes: vec![[0.98, 0.05, -0.05, 0.02, 0., 0.]],
                    centre: [0.47, 0.5]
                }),
                vignette: Some(DngVignette { k: [0.1, 0.2, 0.3, 0.4, 0.5], centre: [0.48, 0.51] }),
            })
        );
        assert_eq!(dng_opcodes(&list(&[opcode(9, &[0; 12])])), None, "no correction opcode");
        // Truncated lists, wrong sizes and counts, and plane counts other than
        // 1 or 3 are malformed.
        for cut in [3, 20, all.len() - 1] {
            assert_eq!(dng_opcodes(&all[..cut]), None, "cut at {cut}");
        }
        let mut more = all.clone();
        more[3] = 4;
        assert_eq!(dng_opcodes(&more), None, "count beyond the list");
        assert_eq!(dng_opcodes(&list(&[opcode(1, &warp[..warp.len() - 8])])), None);
        assert_eq!(dng_opcodes(&list(&[opcode(3, &[&vignette[..], &[0; 8]].concat())])), None);
        let mut two = warp.clone();
        two[3] = 2;
        assert_eq!(dng_opcodes(&list(&[opcode(1, &two)])), None);
        let nan =
            [&1u32.to_be_bytes()[..], &doubles(&[f64::NAN, 0., 0., 0., 0., 0., 0.5, 0.5])].concat();
        assert_eq!(dng_opcodes(&list(&[opcode(1, &nan)])), None);
    }

    #[test]
    fn values_decode_from_either_byte_order() {
        assert_eq!(unit(Value::SRATIONAL), Some((8, 4)));
        assert_eq!(unit(5), None, "unsigned rationals are not a type these tags use");
        let shorts =
            |big_endian, bytes: [u8; 4]| Value::decode(Value::SSHORT, big_endian, bytes.into());
        assert_eq!(shorts(true, [0xff, 0xfe, 0x00, 0x02]), Value::SShort(vec![-2, 2]));
        assert_eq!(shorts(false, [0xfe, 0xff, 0x02, 0x00]), Value::SShort(vec![-2, 2]));
        let bytes = [&(-967i32).to_be_bytes()[..], &2735i32.to_be_bytes()].concat();
        assert_eq!(
            Value::decode(Value::SRATIONAL, true, bytes),
            Value::SRational(vec![[-967, 2735]])
        );
        assert_eq!(Value::decode(Value::UNDEFINED, true, vec![1, 2]), Value::Bytes(vec![1, 2]));
    }
}
