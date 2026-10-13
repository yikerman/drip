use super::{Rect, RgbField, RgbFieldDesc, Stage};
use crate::{
    Error, Result,
    node::color::{self, Mat3},
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext},
};
pub use transform::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    /// Output width over height.
    #[param(default = 1.5, range = 0.1..=10.0)]
    pub ratio: f64,
}
#[derive(Clone, crate::Choice)]
pub enum Aspect {
    #[default]
    #[choice("corners")]
    #[label("From corners")]
    Corners,
    #[choice("ratio")]
    #[label("Ratio")]
    Ratio(Ratio),
}

/// The output's corners on the input, normalized to the input field's domain
/// (0 to 1 across its width and height), and the output aspect.
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Where the output's top-left corner lies on the input, across its width.
    #[param(default = 0.0, range = -1.0..=2.0)]
    #[label("Top-left x")]
    pub top_left_x: f64,
    /// Where the output's top-left corner lies on the input, down its height.
    #[param(default = 0.0, range = -1.0..=2.0)]
    #[label("Top-left y")]
    pub top_left_y: f64,
    #[param(default = 1.0, range = -1.0..=2.0)]
    #[label("Top-right x")]
    pub top_right_x: f64,
    #[param(default = 0.0, range = -1.0..=2.0)]
    #[label("Top-right y")]
    pub top_right_y: f64,
    #[param(default = 1.0, range = -1.0..=2.0)]
    #[label("Bottom-right x")]
    pub bottom_right_x: f64,
    #[param(default = 1.0, range = -1.0..=2.0)]
    #[label("Bottom-right y")]
    pub bottom_right_y: f64,
    #[param(default = 0.0, range = -1.0..=2.0)]
    #[label("Bottom-left x")]
    pub bottom_left_x: f64,
    #[param(default = 1.0, range = -1.0..=2.0)]
    #[label("Bottom-left y")]
    pub bottom_left_y: f64,
    /// Output width over height: from the corners' mean edge lengths, or a ratio.
    #[param]
    pub aspect: Aspect,
}
fn validate(p: &Settings) -> Result<()> {
    if !p.quad().as_flattened().iter().all(|v| v.is_finite()) {
        return Err(Error::Contract("corners must be finite".into()));
    }
    if let Aspect::Ratio(Ratio { ratio }) = p.aspect
        && !(ratio.is_finite() && ratio > 0.0)
    {
        return Err(Error::Contract("aspect ratio must be finite and positive".into()));
    }
    square_to_quad(p.quad())
        .map(drop)
        .ok_or_else(|| Error::Contract("three corners on one line define no transform".into()))
}

impl Settings {
    /// Corners in output order: top-left, top-right, bottom-right, bottom-left.
    pub fn quad(&self) -> [[f64; 2]; 4] {
        [
            [self.top_left_x, self.top_left_y],
            [self.top_right_x, self.top_right_y],
            [self.bottom_right_x, self.bottom_right_y],
            [self.bottom_left_x, self.bottom_left_y],
        ]
    }
    pub fn with_quad(&self, [tl, tr, br, bl]: [[f64; 2]; 4], aspect: Aspect) -> Self {
        Self {
            top_left_x: tl[0],
            top_left_y: tl[1],
            top_right_x: tr[0],
            top_right_y: tr[1],
            bottom_right_x: br[0],
            bottom_right_y: br[1],
            bottom_left_x: bl[0],
            bottom_left_y: bl[1],
            aspect,
        }
    }
}

/// The projective map taking the unit square's corners (0, 0), (1, 0),
/// (1, 1), (0, 1) to `q` in order, in closed form [1, sec. 2.2.3]. Four points
/// with no three collinear determine exactly one such map; otherwise `None`.
pub fn square_to_quad(q: [[f64; 2]; 4]) -> Option<Mat3> {
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    if (0..4).any(|i| cross(q[i], q[(i + 1) % 4], q[(i + 2) % 4]) == 0.0) {
        return None;
    }
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = q;
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
    let den = dx1 * dy2 - dx2 * dy1;
    let g = (sx * dy2 - dx2 * sy) / den;
    let h = (dx1 * sy - sx * dy1) / den;
    Some([
        [x1 - x0 + g * x1, x3 - x0 + h * x3, x0],
        [y1 - y0 + g * y1, y3 - y0 + h * y3, y0],
        [g, h, 1.0],
    ])
}

/// The output's domain and its map into the input plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub map: Mat3,
    pub domain: Rect,
}

/// The quad in the input plane of a field whose domain is `input`.
pub fn plane_quad(p: &Settings, input: &Rect) -> [[f64; 2]; 4] {
    p.quad()
        .map(|[x, y]| [input.origin[0] + x * input.size[0], input.origin[1] + y * input.size[1]])
}

/// The output is `aspect × 1` scaled so its height is the quad's mean side
/// length: corners of a rectangular quad keep their pixel density.
pub fn frame(p: &Settings, input: &Rect) -> Result<Frame> {
    let q = plane_quad(p, input);
    let length = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let height = (length(q[0], q[3]) + length(q[1], q[2])) / 2.0;
    let width = match p.aspect {
        Aspect::Corners => (length(q[0], q[1]) + length(q[3], q[2])) / 2.0,
        Aspect::Ratio(Ratio { ratio }) => ratio * height,
    };
    let square = square_to_quad(q)
        .ok_or_else(|| Error::Contract("three corners on one line define no transform".into()))?;
    let unit = [[1.0 / width, 0.0, 0.0], [0.0, 1.0 / height, 0.0], [0.0, 0.0, 1.0]];
    Ok(Frame {
        map: color::mul(&square, &unit),
        domain: Rect { origin: [0.0, 0.0], size: [width, height] },
    })
}

fn contract(_: &GlobalContext, p: &Settings, field: &RgbFieldDesc) -> Result<(RgbFieldDesc,)> {
    let Frame { map, domain } = frame(p, &field.domain)?;
    let maps = field.maps.each_ref().map(|chain| chain.after(Stage::Projective(map)));
    Ok((RgbFieldDesc { domain, maps, ..field.clone() },))
}

/// Crop, rotate and correct perspective in one map: the output is the
/// content of a four-corner region of the input, made rectangular. Corners
/// are fractions of the input's width and height, so the default corners
/// leave the image unchanged, an inset rectangle crops and a rotated one
/// straightens. Every perspective transform corresponds to exactly one set of
/// corners and aspect. Aspect from corners averages opposite sides; set a
/// ratio for a photographed rectangle whose true proportions differ. Three
/// corners on a line are rejected. Regions outside the input, and anything
/// beyond the horizon a crossed or concave quad puts inside the frame, are
/// black. Nothing is resampled here; a later Sample filters once.
#[crate::node(id = "perspective-transform", name = "Perspective Transform", tags = [tag::GEOMETRY], contract = contract, references = [
    ("[1] Heckbert, Fundamentals of texture mapping and image warping", "https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html"),
])]
pub fn transform(
    _: &KernelContext<'_>,
    _: &Settings,
    field: Read<'_, Device<RgbField>>,
    output: Forward<'_, Device<RgbField>>,
) -> Result<()> {
    output.set(field.data.clone());
    Ok(())
}

/// What Fit keeps while it enlarges the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The current aspect.
    OriginalFormat,
    /// Any aspect from 1:10 to 10:1.
    LargestArea,
}

/// The same projective map with the largest output rectangle inside the
/// input domain, or `None` when the input's image is unbounded (the map's
/// vanishing line crosses it). The aspect is returned as a ratio, since
/// corners of the new frame no longer average to it.
pub fn fit(p: &Settings, input: &Rect, mode: Fit) -> Option<Settings> {
    let Frame { map, domain } = frame(p, input).ok()?;
    let inverse = color::inverse(&map);
    let [ox, oy] = input.origin;
    let [w, h] = input.size;
    let corners = [[ox, oy], [ox + w, oy], [ox + w, oy + h], [ox, oy + h]];
    let mut region = [[0.0; 2]; 4];
    for (corner, point) in corners.iter().zip(&mut region) {
        let [x, y, z] = color::apply(&inverse, [corner[0], corner[1], 1.0]);
        // NaN too: a point on or beyond the vanishing line has no image.
        if z.is_nan() || z <= 0.0 {
            return None;
        }
        *point = [x / z, y / z];
    }
    let current = domain.size[0] / domain.size[1];
    let (aspect, [cx, cy, s]) = match mode {
        Fit::OriginalFormat => (current, inscribed(&region, current)?),
        Fit::LargestArea => largest_area(&region)?,
    };
    let rect = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
        .map(|[dx, dy]| [cx + dx * s * aspect / 2.0, cy + dy * s / 2.0]);
    let quad = rect.map(|[x, y]| {
        let [x, y, z] = color::apply(&map, [x, y, 1.0]);
        [(x / z - ox) / w, (y / z - oy) / h]
    });
    Some(p.with_quad(quad, Aspect::Ratio(Ratio { ratio: aspect })))
}

/// The largest `aspect × 1` rectangle, scaled by `s` about `(cx, cy)`, inside
/// the convex quad `region`: a linear program in `(cx, cy, s)`, since each
/// rectangle corner is linear in them. Its optimum is a vertex where three
/// constraints (a corner on an edge) meet, so every such vertex is tried.
fn inscribed(region: &[[f64; 2]; 4], aspect: f64) -> Option<[f64; 3]> {
    let orientation = {
        let [a, b, c] = [region[0], region[1], region[2]];
        ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).signum()
    };
    // n · (cx, cy, s) ≤ k for each corner and edge.
    let mut constraints = Vec::new();
    for i in 0..4 {
        let (a, b) = (region[i], region[(i + 1) % 4]);
        // Inside when orientation · cross(b − a, p − a) ≥ 0.
        let normal = [orientation * (b[1] - a[1]), -orientation * (b[0] - a[0])];
        let k = normal[0] * a[0] + normal[1] * a[1];
        for [dx, dy] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            let ds = normal[0] * dx * aspect / 2.0 + normal[1] * dy / 2.0;
            constraints.push(([normal[0], normal[1], ds], k));
        }
    }
    let scale = region.as_flattened().iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let feasible = |x: [f64; 3]| {
        x[2] >= 0.0
            && constraints
                .iter()
                .all(|(n, k)| n[0] * x[0] + n[1] * x[1] + n[2] * x[2] <= k + 1e-9 * scale)
    };
    let mut best: Option<[f64; 3]> = None;
    for i in 0..constraints.len() {
        for j in i + 1..constraints.len() {
            for l in j + 1..constraints.len() {
                let rows = [constraints[i].0, constraints[j].0, constraints[l].0];
                if color::determinant(&rows).abs() < 1e-12 * scale * scale {
                    continue;
                }
                let x = color::apply(
                    &color::inverse(&rows),
                    [constraints[i].1, constraints[j].1, constraints[l].1],
                );
                if feasible(x) && best.is_none_or(|b| x[2] > b[2]) {
                    best = Some(x);
                }
            }
        }
    }
    best.filter(|b| b[2] > 0.0)
}

/// Scan the aspect on a log scale, then refine around the best by golden
/// section; the area need not be unimodal over the whole range.
fn largest_area(region: &[[f64; 2]; 4]) -> Option<(f64, [f64; 3])> {
    let area = |log: f64| {
        let aspect = log.exp();
        inscribed(region, aspect).map(|x| (aspect, x, x[2] * x[2] * aspect))
    };
    let (lo, hi) = (0.1f64.ln(), 10.0f64.ln());
    let steps = 64;
    let step = (hi - lo) / steps as f64;
    let best = (0..=steps)
        .filter_map(|i| area(lo + i as f64 * step).map(|r| (i, r)))
        .max_by(|a, b| a.1.2.total_cmp(&b.1.2))?;
    let (mut a, mut b) = (lo + (best.0 as f64 - 1.0) * step, lo + (best.0 as f64 + 1.0) * step);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let value = |x: f64| area(x.clamp(lo, hi)).map_or(0.0, |r| r.2);
    for _ in 0..60 {
        let (c, d) = (b - ratio * (b - a), a + ratio * (b - a));
        if value(c) >= value(d) {
            b = d;
        } else {
            a = c;
        }
    }
    let refined = area(((a + b) / 2.0).clamp(lo, hi))?;
    let best = if refined.2 >= best.1.2 { refined } else { best.1 };
    Some((best.0, best.1))
}

/// Whether the output lies inside the input: a convex quad with every corner
/// within the input domain, allowing for rounding of fitted corners.
pub fn inside(p: &Settings) -> bool {
    let q = p.quad();
    let cross = |i: usize| {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    };
    let turns = (0..4).map(cross);
    let convex = turns.clone().all(|t| t > 0.0) || turns.clone().all(|t| t < 0.0);
    convex && q.iter().flatten().all(|v| (-1e-9..=1.0 + 1e-9).contains(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Rect {
        Rect { origin: [0.0, 0.0], size: [400.0, 300.0] }
    }

    fn project(m: &Mat3, [x, y]: [f64; 2]) -> [f64; 2] {
        let [x, y, z] = color::apply(m, [x, y, 1.0]);
        [x / z, y / z]
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn default_corners_are_the_identity() {
        let Frame { map, domain } = frame(&Settings::default(), &input()).unwrap();
        assert_eq!(domain, input());
        for (i, row) in map.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                assert!((v - f64::from(u8::from(i == j))).abs() < 1e-15, "{map:?}");
            }
        }
    }

    #[test]
    fn an_inset_rectangle_crops_without_scaling() {
        let p = Settings::default()
            .with_quad([[0.25, 0.0], [0.75, 0.0], [0.75, 0.5], [0.25, 0.5]], Aspect::Corners);
        let Frame { map, domain } = frame(&p, &input()).unwrap();
        assert_eq!(domain.size, [200.0, 150.0]);
        assert_eq!(map, [[1.0, 0.0, 100.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn output_corners_land_on_the_quad() {
        let quad = [[0.1, 0.05], [0.95, 0.2], [0.8, 0.9], [-0.1, 0.7]];
        for aspect in [Aspect::Corners, Aspect::Ratio(Ratio { ratio: 0.8 })] {
            let p = Settings::default().with_quad(quad, aspect);
            let Frame { map, domain } = frame(&p, &input()).unwrap();
            let [w, h] = domain.size;
            let expected = plane_quad(&p, &input());
            for (corner, expected) in [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]].iter().zip(expected)
            {
                assert!(close(project(&map, *corner), expected));
            }
        }
    }

    #[test]
    fn corners_recover_any_projective_map() {
        // A map and an output rectangle give corners; the corners and the
        // rectangle's aspect give the same map back, up to scale.
        let map = [[0.9, 0.1, 30.0], [-0.05, 1.1, 12.0], [2e-4, -1e-4, 1.0]];
        let (w, h) = (250.0, 160.0);
        let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]].map(|c| {
            let [x, y] = project(&map, c);
            [x / 400.0, y / 300.0]
        });
        let p = Settings::default().with_quad(corners, Aspect::Ratio(Ratio { ratio: w / h }));
        let Frame { map: recovered, domain } = frame(&p, &input()).unwrap();
        let scale = domain.size[1] / h;
        for (x, y) in [(0.0, 0.0), (w, 0.0), (37.0, 91.0), (w, h)] {
            assert!(close(project(&recovered, [x * scale, y * scale]), project(&map, [x, y])));
        }
    }

    #[test]
    fn collinear_corners_are_rejected() {
        let p = Settings::default()
            .with_quad([[0.0, 0.0], [0.5, 0.5], [1.0, 1.0], [0.0, 1.0]], Aspect::Corners);
        assert!(validate(&p).is_err());
        assert!(frame(&p, &input()).is_err());
        assert!(validate(&Settings::default()).is_ok());
        // Crossed quads are valid maps.
        let crossed = Settings::default()
            .with_quad([[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]], Aspect::Corners);
        assert!(validate(&crossed).is_ok());
        assert!(!inside(&crossed));
    }

    fn rotated(angle: f64) -> Settings {
        let (s, c) = angle.sin_cos();
        let (cx, cy) = (200.0, 150.0);
        let corners = [[0.0, 0.0], [400.0, 0.0], [400.0, 300.0], [0.0, 300.0]].map(|[x, y]| {
            let (dx, dy) = (x - cx, y - cy);
            [(cx + c * dx - s * dy) / 400.0, (cy + s * dx + c * dy) / 300.0]
        });
        Settings::default().with_quad(corners, Aspect::Corners)
    }

    #[test]
    fn fit_keeps_the_map_and_touches_the_input() {
        let p = rotated(0.15);
        assert!(!inside(&p));
        let before = frame(&p, &input()).unwrap();
        for mode in [Fit::OriginalFormat, Fit::LargestArea] {
            let fitted = fit(&p, &input(), mode).unwrap();
            assert!(inside(&fitted), "{mode:?}: {:?}", fitted.quad());
            let after = frame(&fitted, &input()).unwrap();
            if mode == Fit::OriginalFormat {
                let ratio = |f: &Frame| f.domain.size[0] / f.domain.size[1];
                assert!((ratio(&after) - ratio(&before)).abs() < 1e-9);
            }
            // The same rotation: directions in the output keep their angle.
            let direction = |f: &Frame| {
                let (a, b) = (project(&f.map, [0.0, 0.0]), project(&f.map, [1.0, 0.0]));
                (b[1] - a[1]).atan2(b[0] - a[0])
            };
            assert!((direction(&after) - direction(&before)).abs() < 1e-9);
            // Maximal: some corner touches the input's edge.
            let q = fitted.quad();
            let touching = q.iter().flatten().any(|v| v.abs() < 1e-7 || (v - 1.0).abs() < 1e-7);
            assert!(touching, "{mode:?}: {q:?}");
        }
        let original = fit(&p, &input(), Fit::OriginalFormat).unwrap();
        let largest = fit(&p, &input(), Fit::LargestArea).unwrap();
        let area = |p: &Settings| {
            let size = frame(p, &input()).unwrap().domain.size;
            size[0] * size[1]
        };
        assert!(area(&largest) >= area(&original) * (1.0 - 1e-9));
    }

    #[test]
    fn fit_of_an_inside_rectangle_fills_the_input() {
        let p = Settings::default()
            .with_quad([[0.2, 0.2], [0.6, 0.2], [0.6, 0.5], [0.2, 0.5]], Aspect::Corners);
        let fitted = fit(&p, &input(), Fit::LargestArea).unwrap();
        for (a, b) in fitted.quad().iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
            assert!(close(*a, b), "{:?}", fitted.quad());
        }
    }

    #[test]
    fn fit_reports_an_unbounded_image() {
        // Sides converging to a point inside the input, so the horizon
        // through it crosses the input.
        let p = Settings::default().with_quad(
            [[0.45, 0.5], [0.55, 0.5], [1.0, 0.6], [0.0, 0.6]],
            Aspect::Ratio(Ratio { ratio: 1.0 }),
        );
        assert!(fit(&p, &input(), Fit::OriginalFormat).is_none());
    }
}
