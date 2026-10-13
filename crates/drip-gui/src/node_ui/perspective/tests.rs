use super::*;
use drip::{
    definition::Typed,
    eval::Evaluator,
    graph::Dag,
    node::{self, data::*},
    runtime::RuntimeContext,
};

const INPUT: Plane = Plane { origin: [0.0, 0.0], size: [400.0, 300.0] };

fn pixels(width: u32, height: u32) -> Vec<[f32; 3]> {
    (0..width * height).map(|i| [i as f32, -(i as f32) / 4.0, (i % 7) as f32]).collect()
}

/// Source → Perspective Transform, prepared as the worker does.
fn prepared(p: Settings, scale: u32) -> (Prepared, Vec<[f32; 3]>) {
    let (width, height) = (6, 4);
    let mut dag = Dag::new();
    let desc = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    let source = node::source::<Rgb>(&mut dag, desc, pixels(width, height)).unwrap();
    let transform = perspective::add(&mut dag, p).unwrap();
    dag.connect(source, transform.image).unwrap();
    let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
    let global = drip::runtime::GlobalContext { scale };
    let id = transform.node;
    let inputs = evaluator.evaluate_inputs(&dag, &global, &[id]).remove(&id).unwrap().unwrap();
    let ctx = PrepareContext { runtime: evaluator.runtime().clone(), global };
    let prepare = <perspective::Definition as GuiNode>::PREPARE.unwrap();
    let node = perspective::Definition::of(dag.node(id).unwrap()).unwrap();
    (
        prepare(node, inputs.bind::<perspective::Definition>().unwrap(), &ctx).unwrap(),
        pixels(width, height),
    )
}

#[test]
fn identity_views_show_the_input_unchanged() {
    let (view, input) = prepared(Settings::default(), 1);
    assert_eq!(view.input, Plane { origin: [0.0, 0.0], size: [6.0, 4.0] });
    assert_eq!((view.view.clone(), view.output.clone()), (view.input.clone(), view.input.clone()));
    for image in [&view.source, &view.framed] {
        assert_eq!((image.width, image.height), (6, 4));
        assert_eq!(&image.pixels[..], &input[..]);
    }
    let (view, _) = prepared(Settings::default(), 2);
    assert_eq!((view.source.width, view.source.height, view.source.requested_scale), (3, 2, 2));
}

#[test]
fn framing_shows_the_whole_straightened_input() {
    let (s, c) = 0.2f64.sin_cos();
    // The input rotated about its centre: the output is a rotated window.
    let quad = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]: [f64; 2]| {
        let (x, y) = (x * 1.5, y * 1.0);
        [(3.0 + c * x - s * y) / 6.0, (2.0 + s * x + c * y) / 4.0]
    });
    let (view, _) = prepared(Settings::default().with_quad(quad, Aspect::Corners), 1);
    let output = &view.output;
    assert!(view.view.origin[0] < 0.0 && view.view.origin[1] < 0.0);
    assert!(view.view.origin[0] + view.view.size[0] > output.size[0]);
    assert!(view.view.origin[1] + view.view.size[1] > output.size[1]);
    assert!(view.framed.width > view.source.width);
}

#[test]
fn view_bounds_stop_at_an_unbounded_input() {
    let p = Settings::default().with_quad(
        [[0.45, 0.5], [0.55, 0.5], [1.0, 0.6], [0.0, 0.6]],
        Aspect::Ratio(Ratio { ratio: 1.0 }),
    );
    let Frame { map, size } = perspective::frame(&p, INPUT.size).unwrap();
    let bounds = view_bounds(&map, &Plane { origin: [0.0, 0.0], size }, &INPUT);
    let [w, h] = size;
    assert_eq!(bounds, Plane { origin: [-w, -h], size: [3.0 * w, 3.0 * h] });
}

fn view(map: Mat3) -> TransformView {
    let image = |w, h| {
        Arc::new(Image {
            width: w,
            height: h,
            requested_scale: 1,
            plane: crate::node_ui::data::Placement::whole([w as f32, h as f32]),
            texels: vec![0; w * h * 8],
        })
    };
    TransformView {
        source: image(4, 3),
        framed: image(4, 3),
        input: INPUT,
        view: INPUT,
        map,
        output: INPUT,
    }
}

fn crop() -> Settings {
    Settings::default()
        .with_quad([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]], Aspect::Corners)
}

fn drag(view: &TransformView, grip: Grip, current: &Settings) -> Drag {
    let frame = view.frame_of(current).unwrap();
    Drag { grip, start: frame.center, frame, quad: current.quad(), map: view.map }
}

#[test]
fn frames_round_trip_through_corners() {
    let view = view(IDENTITY);
    let frame = view.frame_of(&crop()).unwrap();
    assert_eq!(frame, Box { center: [200.0, 150.0], angle: 0.0, half: [100.0, 75.0] });
    assert_eq!(view.quad_of(&IDENTITY, &frame), crop().quad());
    let rotated = frame.rotated(0.3);
    assert!((Box::of(rotated.corners()).unwrap().angle - 0.3).abs() < 1e-12);
    // A perspective edit not yet shown is not a rectangle in the shown plane.
    let skewed =
        crop().with_quad([[0.2, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]], Aspect::Corners);
    assert!(view.frame_of(&skewed).is_none());
}

#[test]
fn handles_resize_move_and_rotate_the_frame() {
    let frame = Box { center: [200.0, 150.0], angle: 0.0, half: [100.0, 75.0] };
    let at = |dx, dy| [200.0 + dx, 150.0 + dy];
    // Right edge out by 20: the left edge stays.
    let right = frame.dragged(Grip::Edge(1), at(0.0, 0.0), at(20.0, 5.0));
    assert_eq!(right, Box { center: [210.0, 150.0], angle: 0.0, half: [110.0, 75.0] });
    // Top-left corner in by (10, 10).
    let corner = frame.dragged(Grip::Corner(0), at(0.0, 0.0), at(10.0, 10.0));
    assert_eq!(corner.corners()[2], frame.corners()[2]);
    assert_eq!(corner.corners()[0], [110.0, 85.0]);
    // An edge cannot pass the opposite one.
    let collapsed = frame.dragged(Grip::Edge(3), at(0.0, 0.0), at(500.0, 0.0));
    assert!((collapsed.half[0] - 0.5).abs() < 1e-12);
    let moved = frame.dragged(Grip::Move, at(0.0, 0.0), at(-5.0, 7.0));
    assert_eq!(moved.center, [195.0, 157.0]);
    let turned = frame.dragged(Grip::Rotate, at(100.0, 0.0), at(0.0, 100.0));
    assert!((turned.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn keep_inside_stops_moves_at_the_edge_and_shrinks_rotations() {
    let view = view(IDENTITY);
    let current = crop();
    let moving = drag(&view, Grip::Move, &current);
    let target = moving.frame.dragged(Grip::Move, moving.start, [moving.start[0] + 300.0, 150.0]);
    let kept = view.settings(&current, &moving, target, true, false);
    let xs = kept.quad().map(|[x, _]| x);
    assert!((xs[1] - 1.0).abs() < 1e-9 && (xs[0] - 0.5).abs() < 1e-9, "{xs:?}");
    let free = view.settings(&current, &moving, target, false, false);
    assert!(!perspective::inside(&free));

    let full = Settings::default();
    let turning = drag(&view, Grip::Rotate, &full);
    let kept = view.settings(&full, &turning, turning.frame.rotated(0.1), true, true);
    assert!(perspective::inside(&kept));
    let size = perspective::frame(&kept, INPUT.size).unwrap().size;
    assert!(size[0] < 400.0 && (size[0] / size[1] - 4.0 / 3.0).abs() < 1e-9);
    // Touching the edge, not merely inside it.
    assert!(kept.quad().iter().flatten().any(|v| v.abs() < 1e-6 || (v - 1.0).abs() < 1e-6));
}

#[test]
fn level_turns_the_nearest_axis_level() {
    let close = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-12;
    assert!(close(level_angle([0.0, 0.0], [10.0, 1.0]), 0.1f64.atan()));
    assert!(close(level_angle([0.0, 0.0], [-10.0, 1.0]), -(0.1f64.atan())));
    assert!(close(level_angle([0.0, 0.0], [1.0, 10.0]), -(0.1f64.atan())));
    assert_eq!(level_angle([1.0, 1.0], [1.0, 1.0]), None);
}

#[test]
fn bisection_finds_the_last_fitting_fraction() {
    assert_eq!(largest(|_| true), 1.0);
    assert!((largest(|t| t <= 0.3) - 0.3).abs() < 1e-9);
}
