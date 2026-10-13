use super::*;
use crate::{
    node::{self, data::*, exposure},
    runtime::KernelContext,
};

/// A view that forces its input.
#[crate::node(id = "test-sink")]
fn sink(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

/// A demand repeated after the frontend read the view's input downloads
/// nothing in the call. At full detail the store serves the input with the
/// host samples the frontend read, so the frontend reads nothing again.
fn a_repeated_demand_downloads_nothing(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let desc = RgbDesc::new(Extent { width: 64, height: 48 }, Coordinates::Space(REC2020_D65));
    let pixels = (0..64 * 48).map(|i| [i as f32 * 1e-3, 0.5, 0.25]).collect::<Vec<_>>();
    let input = node::source::<Rgb>(&mut dag, desc, pixels).unwrap();
    let brighter = exposure::add(&mut dag, exposure::Exposure { ev: 1.0 }).unwrap();
    dag.connect(input, brighter.image).unwrap();
    let view = sink::add(&mut dag, ()).unwrap();
    dag.connect(brighter.output, view.image).unwrap();
    // The second call's statistics and the downloads of the frontend's
    // read after it.
    let twice = |scale| {
        let evaluator = Evaluator::new(runtime.clone());
        let global = GlobalContext { scale };
        let read = || {
            let mut batch =
                evaluator.evaluate_batch(&dag, &global, &[view.node.into()], Policy::Wait, None);
            let values = batch.values.remove(&view.node).unwrap().unwrap();
            let downloads = runtime.transfers().1;
            values.bind::<sink::Definition>().unwrap().image.data.host().unwrap();
            (batch.statistics, runtime.transfers().1 - downloads)
        };
        assert_eq!(read().1, 1);
        read()
    };
    let (second, read_again) = twice(1);
    assert_eq!((second.nodes, second.hits, second.downloads, read_again), (1, 1, 0, 0));
    let (second, _) = twice(4);
    assert_eq!(second.downloads, 0, "{second:?}");
}

#[cfg(feature = "cpu")]
#[test]
fn cpu_repeated_demand() {
    a_repeated_demand_downloads_nothing(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_repeated_demand() {
    a_repeated_demand_downloads_nothing(RuntimeContext::from_env().unwrap());
}
