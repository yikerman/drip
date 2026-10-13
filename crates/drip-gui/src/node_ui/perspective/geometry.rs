//! The frame tools' geometry in the output plane: the rotated frame being
//! edited, what a drag holds, and the searches Keep inside and Level use.

/// What a frame gesture holds.
#[derive(Clone, Copy)]
pub(super) enum Grip {
    Corner(usize),
    Edge(usize),
    Move,
    Rotate,
    Level,
    QuadCorner(usize),
}

/// A rotated rectangle in the output plane: the frame being edited.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Box {
    pub center: [f64; 2],
    pub angle: f64,
    pub half: [f64; 2],
}

impl Box {
    /// The rectangle with corners `p` (top-left, top-right, bottom-right,
    /// bottom-left), if they form one.
    pub(super) fn of(p: [[f64; 2]; 4]) -> Option<Self> {
        let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
        let (u, v) = (sub(p[1], p[0]), sub(p[3], p[0]));
        let (w, h) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
        let scale = w.max(h);
        let opposite = sub(sub(p[2], p[1]), v);
        let square = (u[0] * v[0] + u[1] * v[1]).abs() <= 1e-6 * scale * scale;
        let closed = opposite[0].hypot(opposite[1]) <= 1e-6 * scale;
        // Mirrored frames would turn handles inside out.
        let upright = u[0] * v[1] - u[1] * v[0] > 0.0;
        (w > 0.0 && h > 0.0 && square && closed && upright).then(|| Self {
            center: [(p[0][0] + p[2][0]) / 2.0, (p[0][1] + p[2][1]) / 2.0],
            angle: u[1].atan2(u[0]),
            half: [w / 2.0, h / 2.0],
        })
    }

    pub(super) fn corners(&self) -> [[f64; 2]; 4] {
        let (s, c) = self.angle.sin_cos();
        let [a, b] = self.half;
        [[-a, -b], [a, -b], [a, b], [-a, b]]
            .map(|[x, y]| [self.center[0] + c * x - s * y, self.center[1] + s * x + c * y])
    }

    /// `p` in this frame's axes, relative to its centre.
    pub(super) fn local(&self, p: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.angle.sin_cos();
        let (x, y) = (p[0] - self.center[0], p[1] - self.center[1]);
        [c * x + s * y, -s * x + c * y]
    }

    pub(super) fn world(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.angle.sin_cos();
        [self.center[0] + c * x - s * y, self.center[1] + s * x + c * y]
    }

    pub(super) fn rotated(&self, by: f64) -> Self {
        Self { angle: self.angle + by, ..*self }
    }

    pub(super) fn scaled(&self, s: f64) -> Self {
        Self { half: self.half.map(|v| v * s), ..*self }
    }

    pub(super) fn towards(&self, target: &Self, t: f64) -> Self {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        Self {
            center: [
                lerp(self.center[0], target.center[0]),
                lerp(self.center[1], target.center[1]),
            ],
            angle: lerp(self.angle, target.angle),
            half: [lerp(self.half[0], target.half[0]), lerp(self.half[1], target.half[1])],
        }
    }

    /// The frame after dragging `grip` from `start` to `at`.
    pub(super) fn dragged(&self, grip: Grip, start: [f64; 2], at: [f64; 2]) -> Self {
        const MIN: f64 = 1.0;
        let (from, to) = (self.local(start), self.local(at));
        let delta = [to[0] - from[0], to[1] - from[1]];
        // Signs of the corners and of each edge's axis, in output order.
        const CORNER: [[f64; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        const EDGE: [[f64; 2]; 4] = [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]];
        let resize = |signs: [f64; 2]| {
            // The opposite side stays put; the dragged side follows the pointer.
            let mut lo = [-self.half[0], -self.half[1]];
            let mut hi = self.half;
            for axis in 0..2 {
                if signs[axis] > 0.0 {
                    hi[axis] = (hi[axis] + delta[axis]).max(lo[axis] + MIN);
                } else if signs[axis] < 0.0 {
                    lo[axis] = (lo[axis] + delta[axis]).min(hi[axis] - MIN);
                }
            }
            let center = self.world([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
            Self { center, half: [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0], ..*self }
        };
        match grip {
            Grip::Corner(i) => resize(CORNER[i]),
            Grip::Edge(i) => resize(EDGE[i]),
            Grip::Move => Self {
                center: [self.center[0] + at[0] - start[0], self.center[1] + at[1] - start[1]],
                ..*self
            },
            Grip::Rotate => {
                let angle = |p: [f64; 2]| (p[1] - self.center[1]).atan2(p[0] - self.center[0]);
                self.rotated(angle(at) - angle(start))
            }
            Grip::Level | Grip::QuadCorner(_) => *self,
        }
    }
}

/// The rotation that makes the line from `a` to `b` horizontal, or vertical
/// when it is steeper than 45°.
pub(super) fn level_angle(a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx.hypot(dy) == 0.0 {
        return None;
    }
    let angle = dy.atan2(dx);
    let quarter = std::f64::consts::FRAC_PI_2;
    Some(angle - (angle / quarter).round() * quarter)
}

/// The largest `t` in `[0, 1]` that `fits`, assuming `fits(0)`.
pub(super) fn largest(fits: impl Fn(f64) -> bool) -> f64 {
    if fits(1.0) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) { lo = mid } else { hi = mid }
    }
    lo
}
