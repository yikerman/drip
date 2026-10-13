//! Where samples sit on an image's full-detail plane.
//!
//! A [`Lattice`] places `extent` samples over a plane of `full` pixels, and a
//! raster stores one [`Rect`] window of it. A preview, a crop and an export
//! band are lattices of the same plane, so a sample's position never depends
//! on how much of the image is stored.

/// Where stored samples sit. `extent` counts the samples of the whole image
/// per axis, and `full` is the size of the plane they cover in full-detail
/// pixels. Sample `i` sits at plane position `(i + 0.5) · full / extent`
/// along each axis. A raster stores the samples in `window`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lattice {
    pub extent: [u32; 2],
    pub full: [u32; 2],
    pub window: Rect,
}
impl Lattice {
    /// The whole lattice of `extent` samples over a plane of `full` pixels.
    pub fn whole(extent: [u32; 2], full: [u32; 2]) -> Self {
        Self { extent, full, window: Rect { origin: [0; 2], extent } }
    }

    /// This lattice holding `window`, which must lie inside it.
    pub fn with(self, window: Rect) -> Self {
        Self { window, ..self }
    }

    pub fn is_whole(&self) -> bool {
        self.window == Rect { origin: [0; 2], extent: self.extent }
    }

    /// The samples whose cells meet `region`, a rectangle of the plane in
    /// full-detail pixels. `None` when they are none.
    pub fn cover(&self, region: Rect) -> Option<Rect> {
        let axis = |a: usize| {
            let step = f64::from(self.full[a]) / f64::from(self.extent[a]);
            let start = (f64::from(region.origin[a]) / step).floor();
            let end = (f64::from(region.origin[a]) + f64::from(region.extent[a])) / step;
            let clamp = |v: f64| v.clamp(0.0, f64::from(self.extent[a])) as u32;
            (clamp(start), clamp(end.ceil()))
        };
        let ((x0, x1), (y0, y1)) = (axis(0), axis(1));
        (x1 > x0 && y1 > y0).then_some(Rect { origin: [x0, y0], extent: [x1 - x0, y1 - y0] })
    }
}

/// An axis-aligned rectangle of whole cells: `extent` cells per axis from
/// `origin`. A region of a plane counts full-detail pixels, and a window of
/// a lattice counts samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub origin: [u32; 2],
    pub extent: [u32; 2],
}
impl Rect {
    /// One past the last cell per axis.
    pub fn end(&self) -> [u32; 2] {
        [0, 1].map(|a| self.origin[a].saturating_add(self.extent[a]))
    }

    pub fn contains(&self, other: &Rect) -> bool {
        let (end, other_end) = (self.end(), other.end());
        (0..2).all(|a| self.origin[a] <= other.origin[a] && other_end[a] <= end[a])
    }

    pub fn cells(&self) -> usize {
        self.extent[0] as usize * self.extent[1] as usize
    }
}
