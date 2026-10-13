//! A meta node expands into a sub-graph of other kinds when evaluation
//! reaches it, and those nodes run and are keyed like any node's. Pyramid
//! blur expands to a Resample and a Sharpen per level and one Resample back,
//! keeps a flat image and the input's lattice at every scale, and a crop
//! through it equals the whole output there.
use drip::{
    Error, Result,
    eval::{Evaluator, Policy, Statistics, Target},
    expand::SubGraph,
    graph::Dag,
    image::Description,
    lattice::Rect,
    node::{self, data::*, pyramid},
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
};
use std::sync::Arc;

fn rgb(width: u32, height: u32) -> RgbDesc {
    RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65))
}

fn blurred(dag: &mut Dag, input: Output<Rgb>, levels: i64) -> Output<Rgb> {
    let blur = pyramid::add(dag, pyramid::Settings { levels, radius: 1.0 }).unwrap();
    dag.connect(input, blur.image).unwrap();
    blur.output
}

/// A view that forces its input.
#[drip::node(id = "test-sink")]
fn sink(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

fn refuse(_: &GlobalContext, _: &(), _: &RgbDesc) -> Result<(RgbDesc,)> {
    Err(Error::Contract("refused".into()))
}

/// A kernel whose contract always fails.
#[drip::node(id = "test-refuse", contract = refuse)]
fn refuser(
    _: &KernelContext<'_>,
    _: &(),
    _image: Read<'_, Rgb>,
    _output: Write<'_, Rgb>,
) -> Result<()> {
    Ok(())
}

/// A meta node whose sub-graph is one `test-refuse` node.
#[drip::node(id = "test-refusing-meta", meta)]
fn refusing(
    _: &(),
    graph: &mut SubGraph,
    image: Inlet<'_, Rgb>,
    output: Outlet<Rgb>,
) -> Result<()> {
    let node = refuser::add(&mut graph.dag, ())?;
    graph.input(image, node.image)?;
    graph.output(node.output, output)
}

/// The expansion holds a Resample and a Sharpen per level and the Resample
/// back, and needs its input's description. A meta node has no contract
/// and does not run itself.
fn the_expansion_has_two_nodes_per_level_and_one_more() {
    let settings = pyramid::Settings { levels: 3, radius: 1.0 };
    let blur = pyramid::definition(settings).unwrap();
    let desc: Description = Arc::new(rgb(64, 48));
    let graph = blur.expand(&[Some(desc.clone())]).unwrap();
    let kinds: Vec<_> = graph.dag.nodes().map(|(_, node)| node.kind().id).collect();
    let expected = ["resample", "sharpen"].repeat(3).into_iter().chain(["resample"]);
    assert_eq!(kinds, expected.collect::<Vec<_>>());
    let missing = blur.expand(&[None]).err().unwrap();
    assert!(matches!(missing, Error::MissingInput { node: "pyramid-blur", port: "image" }));
    assert!(blur.contract(&GlobalContext::default(), &[Some(desc)]).is_err());
}

fn evaluate(
    runtime: &RuntimeContext,
    evaluator: Option<&Evaluator>,
    dag: &Dag,
    scale: u32,
    output: Output<Rgb>,
) -> (RgbDesc, Vec<[f32; 3]>, Statistics) {
    let own = Evaluator::new(runtime.clone());
    let evaluator = evaluator.unwrap_or(&own);
    let result = evaluator.evaluate::<Rgb>(dag, &GlobalContext { scale }, output).unwrap();
    (result.desc, result.data.host().unwrap().to_vec(), result.statistics)
}

/// Every level and the Resample back average with normalized weights, so a
/// flat image passes unchanged and keeps its lattice. The source, the meta
/// node and its seven nodes run once, and the same demand again hits the
/// stored output.
fn a_flat_image_passes_unchanged(runtime: RuntimeContext) {
    let (width, height) = (37, 29);
    let flat = [0.3f32, 1.7, -0.2];
    let mut dag = Dag::new();
    let input = node::source::<Rgb>(&mut dag, rgb(width, height), vec![flat; 37 * 29]).unwrap();
    let output = blurred(&mut dag, input, 3);
    let evaluator = Evaluator::new(runtime.clone());
    let (desc, pixels, statistics) = evaluate(&runtime, Some(&evaluator), &dag, 1, output);
    assert_eq!((desc.extent, desc.full), (Extent { width, height }, Extent { width, height }));
    for px in &pixels {
        for (v, expected) in px.iter().zip(flat) {
            // Seven stages of normalized f32 weights, with WGSL's `exp`
            // and division accurate to a few ulps each.
            assert!((v - expected).abs() <= 1e-5 * expected.abs().max(1.0), "{px:?}");
        }
    }
    assert_eq!(statistics.nodes, 9, "{statistics:?}");
    let again = evaluate(&runtime, Some(&evaluator), &dag, 1, output).2;
    assert_eq!((again.nodes, again.hits), (0, 1), "{again:?}");
    let (desc, _, _) = evaluate(&runtime, None, &dag, 4, output);
    assert_eq!(desc.extent, Extent { width: 10, height: 8 });
}

/// `sink`'s input over `region`, as bits, with the call's statistics.
fn view(
    runtime: &RuntimeContext,
    dag: &Dag,
    sink: NodeId,
    region: Option<Rect>,
) -> (Vec<u32>, Statistics) {
    let target = Target { node: sink, region };
    let evaluator = Evaluator::new(runtime.clone());
    let mut batch =
        evaluator.evaluate_batch(dag, &GlobalContext::default(), &[target], Policy::Wait, None);
    let values = batch.inputs.remove(&sink).unwrap().unwrap();
    let image = values.bind::<sink::Definition>().unwrap().image;
    let bits = image.data.host().unwrap().iter().flat_map(|px| px.map(f32::to_bits)).collect();
    (bits, batch.statistics)
}

/// A crop through the pyramid equals the whole output's samples bit for
/// bit and computes fewer samples: each level computes the part the level
/// above reads.
fn a_crop_through_the_pyramid_equals_the_whole_output(runtime: RuntimeContext) {
    let (width, height) = (96u32, 80u32);
    let pixels: Vec<_> = (0..width * height)
        .map(|i| {
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            [(x * 0.37).sin() * 2.0 + y * 0.01, (y * 0.29).cos() + x * 0.02, (x - y) * 0.03]
        })
        .collect();
    let mut dag = Dag::new();
    let input = node::source::<Rgb>(&mut dag, rgb(width, height), pixels).unwrap();
    let output = blurred(&mut dag, input, 2);
    let shown = sink::add(&mut dag, ()).unwrap();
    dag.connect(output, shown.image).unwrap();
    let (whole, all) = view(&runtime, &dag, shown.node, None);
    let window = Rect { origin: [10, 52], extent: [24, 16] };
    let (crop, part) = view(&runtime, &dag, shown.node, Some(window));
    let expected: Vec<_> = (window.origin[1]..window.end()[1])
        .flat_map(|y| (window.origin[0]..window.end()[0]).map(move |x| (y * width + x) as usize))
        .flat_map(|i| whole[i * 3..i * 3 + 3].to_vec())
        .collect();
    assert_eq!(crop, expected);
    assert!(part.samples < all.samples / 2, "{part:?} against {all:?}");
}

/// Each node of a sub-graph reports its own timing, and a failure inside a
/// sub-graph names the node it happened on.
fn sub_graph_nodes_report_their_timings_and_failures(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let input = node::source::<Rgb>(&mut dag, rgb(16, 12), vec![[0.5; 3]; 16 * 12]).unwrap();
    let blur = blurred(&mut dag, input, 2);
    let shown = sink::add(&mut dag, ()).unwrap();
    dag.connect(blur, shown.image).unwrap();
    let meta = refusing::add(&mut dag, ()).unwrap();
    dag.connect(input, meta.image).unwrap();
    let refused = sink::add(&mut dag, ()).unwrap();
    dag.connect(meta.output, refused.image).unwrap();
    let targets = [shown.node.into(), refused.node.into()];
    let mut timings = Vec::new();
    let evaluator = Evaluator::new(runtime);
    let global = GlobalContext::default();
    let mut batch =
        evaluator.evaluate_batch(&dag, &global, &targets, Policy::Wait, Some(&mut timings));
    let count = |kind| timings.iter().filter(|timing| timing.kind == kind).count();
    assert_eq!((count("resample"), count("sharpen"), count("pyramid-blur")), (3, 2, 1));
    assert!(timings.iter().any(|timing| timing.kind == "test-refuse" && !timing.success));
    assert!(batch.inputs.remove(&shown.node).unwrap().is_ok());
    let error = batch.inputs.remove(&refused.node).unwrap().err().unwrap();
    let Error::Node { kind: "test-refusing-meta", source, .. } = error else { panic!("{error}") };
    assert!(matches!(*source, Error::Node { kind: "test-refuse", .. }), "{source}");
}

/// Two meta nodes with equal keys in one demand expand to the same
/// sub-graph, and both receive its output.
fn equal_meta_nodes_share_their_expansion(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let pixels = (0..20 * 14).map(|i| [i as f32 * 0.01, 0.5, 1.0]).collect::<Vec<_>>();
    let input = node::source::<Rgb>(&mut dag, rgb(20, 14), pixels).unwrap();
    let sinks: Vec<NodeId> = (0..2)
        .map(|_| {
            let shown = sink::add(&mut dag, ()).unwrap();
            let blur = blurred(&mut dag, input, 2);
            dag.connect(blur, shown.image).unwrap();
            shown.node
        })
        .collect();
    let targets: Vec<Target> = sinks.iter().map(|&node| node.into()).collect();
    let evaluator = Evaluator::new(runtime);
    let global = GlobalContext::default();
    let mut batch = evaluator.evaluate_batch(&dag, &global, &targets, Policy::Wait, None);
    let mut pixels = sinks.iter().map(|node| {
        let values = batch.inputs.remove(node).unwrap().unwrap();
        let image = values.bind::<sink::Definition>().unwrap().image;
        image.data.host().unwrap().iter().flat_map(|px| px.map(f32::to_bits)).collect::<Vec<_>>()
    });
    assert_eq!(pixels.next(), pixels.next());
}

fn all(runtime: RuntimeContext) {
    the_expansion_has_two_nodes_per_level_and_one_more();
    a_flat_image_passes_unchanged(runtime.clone());
    a_crop_through_the_pyramid_equals_the_whole_output(runtime.clone());
    sub_graph_nodes_report_their_timings_and_failures(runtime.clone());
    equal_meta_nodes_share_their_expansion(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_meta() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_meta() {
    all(RuntimeContext::from_env().unwrap());
}
