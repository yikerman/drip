//! Perspective Transform's editing view (agent-docs/FIELD.md, "Editing
//! view"). Preparation warps the transform's input twice through the
//! library's resampling: as it is, for placing corners, and through the
//! current map over the input's whole image, for framing. Handles are drawn
//! and edited through the map the shown image was warped with, so they stay
//! on its pixels while a newer evaluation is pending.

use super::{ControlCx, GuiNode, Prepare, data::HostImage, data::PrepareContext, image_view};
use crate::editing::NodeCx;
use crate::render::{
    image::Image,
    node_views::{Drawable, Images, IntoDrawable},
};
use crate::theme::{self, Tip};
use drip::{
    body::Warp,
    node::{
        color::{self, Mat3},
        data::{Extent, Rgb, RgbDesc},
        filter::Filter,
        perspective::{self, Aspect, Fit, Frame, Ratio, Settings},
    },
    ports::Read,
};
use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use std::sync::Arc;

/// Longest side of a prepared image, in samples.
const MAX_SIDE: u32 = 4096;
/// Pointer distance, in points, within which a handle is grabbed.
const GRAB: f32 = 10.0;
const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl GuiNode for perspective::Definition {
    type Presentation = Prepared;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Inputs(|node, inputs, ctx| prepare(&node.parameters, &inputs.image, ctx)));
}

/// An axis-aligned rectangle in a plane, in full-detail pixels.
#[derive(Clone, Debug, PartialEq)]
struct Plane {
    origin: [f64; 2],
    size: [f64; 2],
}

pub struct Prepared {
    source: HostImage,
    input: Plane,
    framed: HostImage,
    view: Plane,
    map: Mat3,
    output: Plane,
}

fn prepare(p: &Settings, image: &Read<'_, Rgb>, ctx: &PrepareContext) -> drip::Result<Prepared> {
    let kernels = ctx.kernels();
    let input = drip::image::Image::from(image.data.clone());
    // The area `plane` of a plane that `map` takes into the input, warped
    // with Lanczos-3 onto at most `MAX_SIDE` samples a side.
    let render = |map: &Mat3, plane: &Plane| -> drip::Result<HostImage> {
        let [w, h] = plane.size.map(|v| v.round().clamp(1.0, f64::from(u32::MAX)) as u32);
        let divisor = ctx.global.scale.max(w.max(h).div_ceil(MAX_SIDE));
        let full = Extent { width: w, height: h };
        let extent = full.reduced(divisor);
        let coordinates = image.desc.coordinates.clone();
        let placement = crate::node_ui::data::Placement::whole([w as f32, h as f32]);
        let desc = RgbDesc { extent: extent.clone(), full, window: None, coordinates };
        // The warp's output plane spans `full`, so place it over `plane` first.
        let [sx, sy] = [plane.size[0] / f64::from(w), plane.size[1] / f64::from(h)];
        let place = [[sx, 0.0, plane.origin[0]], [0.0, sy, plane.origin[1]], [0.0, 0.0, 1.0]];
        let warp = Warp::filtered([color::mul(map, &place); 3], Filter::Lanczos3.body());
        let warped = input.warp::<Rgb>(&warp, Arc::new(desc))?.raster::<Rgb>(&kernels)?;
        Ok(HostImage {
            width: extent.width as usize,
            height: extent.height as usize,
            requested_scale: ctx.global.scale,
            color_space: image.desc.coordinates.to_string(),
            pixels: warped.host()?.clone(),
            plane: placement,
        })
    };
    let input_plane = Plane { origin: [0.0, 0.0], size: perspective::plane(image.desc) };
    let Frame { map, size } = perspective::frame(p, input_plane.size)?;
    let output = Plane { origin: [0.0, 0.0], size };
    let view = view_bounds(&map, &output, &input_plane);
    Ok(Prepared {
        source: render(&IDENTITY, &input_plane)?,
        framed: render(&map, &view)?,
        input: input_plane,
        view,
        map,
        output,
    })
}

/// The output plane's area worth showing: the input's image and the output,
/// at most one output size beyond the output on each side, which also bounds
/// an input whose image is unbounded.
fn view_bounds(map: &Mat3, output: &Plane, input: &Plane) -> Plane {
    let [ox, oy] = output.origin;
    let [w, h] = output.size;
    let limit = [[ox - w, oy - h], [ox + 2.0 * w, oy + 2.0 * h]];
    let inverse = color::inverse(map);
    let mut lo = [ox, oy];
    let mut hi = [ox + w, oy + h];
    for corner in corners(input) {
        match project(&inverse, corner) {
            Some(p) => {
                for i in 0..2 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            None => (lo, hi) = (limit[0], limit[1]),
        }
    }
    let lo = [0, 1].map(|i| lo[i].max(limit[0][i]));
    let hi = [0, 1].map(|i| hi[i].min(limit[1][i]));
    Plane { origin: lo, size: [hi[0] - lo[0], hi[1] - lo[1]] }
}

impl IntoDrawable for Prepared {
    fn into_drawable(self, images: &mut Images) -> Arc<dyn Drawable> {
        Arc::new(TransformView {
            source: images.pack(&self.source),
            framed: images.pack(&self.framed),
            input: self.input,
            view: self.view,
            map: self.map,
            output: self.output,
        })
    }
}

pub struct TransformView {
    source: Arc<Image>,
    framed: Arc<Image>,
    /// The input domain, which `source` spans.
    input: Plane,
    /// The output-plane area `framed` spans.
    view: Plane,
    /// Output plane to input plane, as `framed` was sampled.
    map: Mat3,
    /// The output domain `map` was built for.
    output: Plane,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Frame,
    Level,
    Perspective,
}

#[derive(Clone, Copy)]
enum Grip {
    Corner(usize),
    Edge(usize),
    Move,
    Rotate,
    Level,
    QuadCorner(usize),
}

/// A gesture in progress. Frame gestures work in the output plane of the
/// image shown when they started, through `map`: a preparation arriving
/// mid-drag has its own plane.
#[derive(Clone, Copy)]
struct Drag {
    grip: Grip,
    /// Pointer at the start, in the gesture's plane.
    start: [f64; 2],
    frame: Box,
    quad: [[f64; 2]; 4],
    map: Mat3,
}

#[derive(Clone, Copy)]
struct State {
    tool: Tool,
    keep_inside: bool,
    drag: Option<Drag>,
    unbounded: bool,
}
impl Default for State {
    fn default() -> Self {
        Self { tool: Tool::Frame, keep_inside: true, drag: None, unbounded: false }
    }
}

/// Plane ↔ screen for an image spanning `plane` drawn at `screen`.
#[derive(Clone, Copy)]
struct Placement {
    plane: [f64; 4],
    screen: Rect,
}
impl Placement {
    fn new(plane: &Plane, screen: Rect) -> Self {
        Self { plane: [plane.origin[0], plane.origin[1], plane.size[0], plane.size[1]], screen }
    }
    fn to_screen(self, [x, y]: [f64; 2]) -> Pos2 {
        let [ox, oy, w, h] = self.plane;
        pos2(
            self.screen.min.x + ((x - ox) / w) as f32 * self.screen.width(),
            self.screen.min.y + ((y - oy) / h) as f32 * self.screen.height(),
        )
    }
    fn to_plane(self, p: Pos2) -> [f64; 2] {
        let [ox, oy, w, h] = self.plane;
        [
            ox + f64::from((p.x - self.screen.min.x) / self.screen.width()) * w,
            oy + f64::from((p.y - self.screen.min.y) / self.screen.height()) * h,
        ]
    }
}

impl Drawable for TransformView {
    fn draw(&self, painter: &Painter, rect: Rect, id: egui::Id, _: &FontId) {
        let size = vec2(self.framed.width as f32, self.framed.height as f32);
        let fit = (rect.width() / size.x).min(rect.height() / size.y);
        let shown = Rect::from_center_size(rect.center(), size * fit);
        crate::render::image::draw(painter, shown, id, self.framed.clone(), true);
        let placement = Placement::new(&self.view, shown);
        let output = corners(&self.output).map(|p| placement.to_screen(p));
        overlay(&painter.with_clip_rect(rect), rect, &output, true);
    }

    fn window(&self, ui: &mut Ui) {
        image_view::show(ui, &self.framed, None, true, false);
    }

    fn interact(&self, ui: &mut Ui, node: &mut NodeCx) {
        let mut node = ControlCx::<Settings>::new(node);
        let current = node.parameters();
        let id = ui.id().with("perspective view");
        let mut state = ui.data(|d| d.get_temp::<State>(id)).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for (tool, label, tip) in [
                (Tool::Frame, "Frame", "Crop, move, and rotate by dragging outside the frame"),
                (Tool::Level, "Level", "Drag along a horizon or vertical to level it"),
                (Tool::Perspective, "Perspective", "Place the corners on the input"),
            ] {
                ui.selectable_value(&mut state.tool, tool, label).tip(tip);
            }
            ui.checkbox(&mut state.keep_inside, "Keep inside")
                .tip("Edits keep the frame inside the input, so no black enters");
            for (mode, label, tip) in [
                (Fit::OriginalFormat, "Fit", "Largest frame of the current aspect"),
                (Fit::LargestArea, "Fit largest", "Largest frame of any aspect"),
            ] {
                if ui.button(label).tip(tip).clicked() {
                    match perspective::fit(&current, self.input.size, mode) {
                        Some(fitted) => node.set(&fitted),
                        None => state.unbounded = true,
                    }
                }
            }
            if state.unbounded {
                ui.weak("No largest frame: the horizon crosses the input");
            }
            ui.weak("Right-drag pans");
        });
        let (image, plane) = match state.tool {
            Tool::Perspective => (&self.source, &self.input),
            Tool::Frame | Tool::Level => (&self.framed, &self.view),
        };
        let Some(shown) = image_view::show(ui, image, None, true, true) else {
            ui.data_mut(|d| d.insert_temp(id, state));
            return;
        };
        let placement = Placement::new(plane, shown.image);
        let painter = ui.painter().with_clip_rect(shown.area);
        let pointer = shown.response.interact_pointer_pos().or(shown.response.hover_pos());
        let edited = match state.tool {
            Tool::Perspective => {
                self.perspective_tool(&painter, &shown, placement, pointer, &current, &mut state)
            }
            Tool::Frame | Tool::Level => {
                self.frame_tool(&painter, &shown, placement, pointer, &current, &mut state)
            }
        };
        if let Some(edited) = edited {
            state.unbounded = false;
            node.set(&edited);
        }
        if shown.response.drag_stopped() {
            state.drag = None;
        }
        ui.data_mut(|d| d.insert_temp(id, state));
    }
}

impl TransformView {
    /// The current corners as a rectangle in the prepared output plane, or
    /// `None` while the shown image predates a perspective edit.
    fn frame_of(&self, p: &Settings) -> Option<Box> {
        let inverse = color::inverse(&self.map);
        let quad = perspective::plane_quad(p, self.input.size);
        let mut points = [[0.0; 2]; 4];
        for (q, point) in quad.iter().zip(&mut points) {
            *point = project(&inverse, *q)?;
        }
        Box::of(points)
    }

    /// Normalized corners of `frame`, a rectangle in the plane `map` sends
    /// to the input.
    fn quad_of(&self, map: &Mat3, frame: &Box) -> [[f64; 2]; 4] {
        let [ox, oy] = self.input.origin;
        let [w, h] = self.input.size;
        frame.corners().map(|corner| {
            let [x, y] = project(map, corner).unwrap_or(corner);
            [(x - ox) / w, (y - oy) / h]
        })
    }

    /// A point of the shown plane in the plane of `map`, through the input.
    fn rebase(&self, map: &Mat3, p: [f64; 2]) -> Option<[f64; 2]> {
        project(&color::inverse(map), project(&self.map, p)?)
    }

    fn frame_tool(
        &self,
        painter: &Painter,
        shown: &image_view::Shown,
        placement: Placement,
        pointer: Option<Pos2>,
        current: &Settings,
        state: &mut State,
    ) -> Option<Settings> {
        let Some(frame) = self.frame_of(current) else {
            painter.text(
                shown.area.left_top() + vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                "Updating…",
                FontId::proportional(theme::BODY_SIZE),
                theme::TEXT,
            );
            return None;
        };
        let screen = frame.corners().map(|p| placement.to_screen(p));
        overlay(painter, shown.area, &screen, true);
        if state.tool == Tool::Frame {
            for handle in screen.iter().copied().chain(edges(&screen)) {
                painter.rect_filled(Rect::from_center_size(handle, vec2(7.0, 7.0)), 0.0, HANDLE);
            }
        }
        let response = &shown.response;
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(at) = pointer
        {
            let grip = match state.tool {
                Tool::Level => Grip::Level,
                _ => grab(&screen, at),
            };
            state.drag = Some(Drag {
                grip,
                start: placement.to_plane(at),
                frame,
                quad: current.quad(),
                map: self.map,
            });
        }
        let drag = state.drag?;
        let at = self.rebase(&drag.map, placement.to_plane(pointer?))?;
        if let Grip::Level = drag.grip {
            let shown = |p| project(&color::inverse(&self.map), p);
            let line = [drag.start, at]
                .map(|p| project(&drag.map, p).and_then(shown).map(|p| placement.to_screen(p)));
            if let [Some(a), Some(b)] = line {
                painter.line_segment([a, b], Stroke::new(2.0, HANDLE));
            }
            if !response.drag_stopped() {
                return None;
            }
            let target = drag.frame.rotated(level_angle(drag.start, at)?);
            return Some(self.settings(current, &drag, target, state.keep_inside, true));
        }
        if !response.dragged_by(egui::PointerButton::Primary) {
            return None;
        }
        let target = drag.frame.dragged(drag.grip, drag.start, at);
        let shrink = matches!(drag.grip, Grip::Rotate);
        Some(self.settings(current, &drag, target, state.keep_inside, shrink))
    }

    /// Settings for `target`, reached from `start`. Keep inside stops a move
    /// or resize at the input's edge, and shrinks a rotated frame about its
    /// centre until it fits.
    fn settings(
        &self,
        current: &Settings,
        drag: &Drag,
        target: Box,
        keep_inside: bool,
        shrink: bool,
    ) -> Settings {
        let start = drag.frame;
        let with = |frame: Box| {
            let ratio = frame.half[0] / frame.half[1];
            current.with_quad(self.quad_of(&drag.map, &frame), Aspect::Ratio(Ratio { ratio }))
        };
        if !keep_inside || !perspective::inside(&with(start)) {
            return with(target);
        }
        let candidate = |t: f64| {
            if shrink { target.scaled(t) } else { start.towards(&target, t) }
        };
        let t = largest(|t| perspective::inside(&with(candidate(t))));
        // Nothing fits when even a vanishing frame would leave the input.
        if t > 0.0 { with(candidate(t)) } else { with(start) }
    }

    fn perspective_tool(
        &self,
        painter: &Painter,
        shown: &image_view::Shown,
        placement: Placement,
        pointer: Option<Pos2>,
        current: &Settings,
        state: &mut State,
    ) -> Option<Settings> {
        let quad = perspective::plane_quad(current, self.input.size);
        let screen = quad.map(|p| placement.to_screen(p));
        overlay(painter, shown.area, &screen, convex(&screen));
        for corner in screen {
            painter.circle_stroke(corner, 6.0, Stroke::new(2.0, HANDLE));
        }
        let response = &shown.response;
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(at) = pointer
            && let Some(i) = nearest(&screen, at)
        {
            state.drag = Some(Drag {
                grip: Grip::QuadCorner(i),
                start: placement.to_plane(at),
                frame: Box::default(),
                quad: current.quad(),
                map: self.map,
            });
        }
        let drag = state.drag?;
        let (Grip::QuadCorner(i), true) =
            (drag.grip, response.dragged_by(egui::PointerButton::Primary))
        else {
            return None;
        };
        let at = placement.to_plane(pointer?);
        let mut quad = drag.quad;
        for axis in 0..2 {
            quad[i][axis] += (at[axis] - drag.start[axis]) / self.input.size[axis];
            if state.keep_inside {
                quad[i][axis] = quad[i][axis].clamp(0.0, 1.0);
            }
        }
        let edited = current.with_quad(quad, current.aspect.clone());
        let valid = perspective::square_to_quad(quad).is_some()
            && (!state.keep_inside || perspective::inside(&edited));
        valid.then_some(edited)
    }
}

const HANDLE: Color32 = Color32::from_rgb(250, 250, 250);

/// Darkens around `frame` (when convex) and outlines it.
fn overlay(painter: &Painter, area: Rect, frame: &[Pos2; 4], darken: bool) {
    if darken {
        let outer = area.expand(area.width().max(area.height()));
        let outer =
            [outer.left_top(), outer.right_top(), outer.right_bottom(), outer.left_bottom()];
        let mut mesh = egui::Mesh::default();
        let shade = Color32::from_black_alpha(140);
        for &p in outer.iter().chain(frame) {
            mesh.colored_vertex(p, shade);
        }
        // Clockwise rings: outer 0..4, frame 4..8.
        let flip = signed_area(frame) < 0.0;
        for i in 0..4u32 {
            let j = (i + 1) % 4;
            let (a, b) = if flip { (4 + (4 - i) % 4, 4 + (4 - j) % 4) } else { (4 + i, 4 + j) };
            mesh.add_triangle(i, j, a);
            mesh.add_triangle(a, j, b);
        }
        painter.add(Shape::mesh(mesh));
    }
    let mut ring = frame.to_vec();
    ring.push(frame[0]);
    painter.add(Shape::line(ring, Stroke::new(1.5, HANDLE)));
}

fn cross(a: egui::Vec2, b: egui::Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn signed_area(p: &[Pos2; 4]) -> f32 {
    (0..4).map(|i| p[i].x * p[(i + 1) % 4].y - p[(i + 1) % 4].x * p[i].y).sum::<f32>() / 2.0
}

fn convex(p: &[Pos2; 4]) -> bool {
    let turn = |i: usize| cross(p[(i + 1) % 4] - p[i], p[(i + 2) % 4] - p[(i + 1) % 4]);
    (0..4).all(|i| turn(i) > 0.0) || (0..4).all(|i| turn(i) < 0.0)
}

/// Midpoints of top, right, bottom and left.
fn edges(p: &[Pos2; 4]) -> [Pos2; 4] {
    std::array::from_fn(|i| p[i].lerp(p[(i + 1) % 4], 0.5))
}

fn nearest(points: &[Pos2; 4], at: Pos2) -> Option<usize> {
    (0..4)
        .map(|i| (i, points[i].distance(at)))
        .filter(|&(_, d)| d <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn grab(frame: &[Pos2; 4], at: Pos2) -> Grip {
    if let Some(i) = nearest(frame, at) {
        Grip::Corner(i)
    } else if let Some(i) = nearest(&edges(frame), at) {
        Grip::Edge(i)
    } else if convex(frame) && inside_polygon(frame, at) {
        Grip::Move
    } else {
        Grip::Rotate
    }
}

fn inside_polygon(p: &[Pos2; 4], at: Pos2) -> bool {
    let side = |i: usize| cross(p[(i + 1) % 4] - p[i], at - p[i]);
    (0..4).all(|i| side(i) >= 0.0) || (0..4).all(|i| side(i) <= 0.0)
}

/// The rotation that makes the line from `a` to `b` horizontal, or vertical
/// when it is steeper than 45°.
fn level_angle(a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx.hypot(dy) == 0.0 {
        return None;
    }
    let angle = dy.atan2(dx);
    let quarter = std::f64::consts::FRAC_PI_2;
    Some(angle - (angle / quarter).round() * quarter)
}

/// The largest `t` in `[0, 1]` that `fits`, assuming `fits(0)`.
fn largest(fits: impl Fn(f64) -> bool) -> f64 {
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

fn corners(r: &Plane) -> [[f64; 2]; 4] {
    let [x, y] = r.origin;
    let [w, h] = r.size;
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

fn project(m: &Mat3, [x, y]: [f64; 2]) -> Option<[f64; 2]> {
    let [x, y, z] = color::apply(m, [x, y, 1.0]);
    (z > 0.0).then(|| [x / z, y / z])
}

/// A rotated rectangle in the output plane: the frame being edited.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Box {
    center: [f64; 2],
    angle: f64,
    half: [f64; 2],
}

impl Box {
    /// The rectangle with corners `p` (top-left, top-right, bottom-right,
    /// bottom-left), if they form one.
    fn of(p: [[f64; 2]; 4]) -> Option<Self> {
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

    fn corners(&self) -> [[f64; 2]; 4] {
        let (s, c) = self.angle.sin_cos();
        let [a, b] = self.half;
        [[-a, -b], [a, -b], [a, b], [-a, b]]
            .map(|[x, y]| [self.center[0] + c * x - s * y, self.center[1] + s * x + c * y])
    }

    /// `p` in this frame's axes, relative to its centre.
    fn local(&self, p: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.angle.sin_cos();
        let (x, y) = (p[0] - self.center[0], p[1] - self.center[1]);
        [c * x + s * y, -s * x + c * y]
    }

    fn world(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.angle.sin_cos();
        [self.center[0] + c * x - s * y, self.center[1] + s * x + c * y]
    }

    fn rotated(&self, by: f64) -> Self {
        Self { angle: self.angle + by, ..*self }
    }

    fn scaled(&self, s: f64) -> Self {
        Self { half: self.half.map(|v| v * s), ..*self }
    }

    fn towards(&self, target: &Self, t: f64) -> Self {
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
    fn dragged(&self, grip: Grip, start: [f64; 2], at: [f64; 2]) -> Self {
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

#[cfg(test)]
mod tests;
