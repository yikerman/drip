//! Test-only node kinds and presentation that the registry and bindings must
//! find, e.g. for project loading or prepared views.
use crate::node_ui::{
    Action, Binding, ControlCx, GuiNode, Prepare,
    data::{HostImage, Placement},
    preview::PreviewImage,
};
use drip::{
    node::data::{Coordinates, Extent, REC2020_D65, Rgb, RgbDesc},
    ports::{Read, Write},
    runtime::KernelContext,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

pub static NODES: &[&drip::definition::Kind] = &[&display::KIND];
pub static BINDINGS: &[Binding] = &[
    Binding::new::<slow::Definition>(),
    Binding::new::<declared_view::Definition>(),
    Binding::new::<custom_controls_view::Definition>(),
    Binding::new::<probe::Definition>(),
    Binding::new::<write::Definition>(),
];

fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
    Ok(())
}

const PIXELS: [[f32; 3]; 4] = [[1.0; 3], [0.18; 3], [0.0, 1.0, 0.0], [0.0; 3]];

fn source_contract(_: &drip::runtime::GlobalContext, _: &()) -> drip::Result<(RgbDesc,)> {
    Ok((RgbDesc::new(Extent { width: 2, height: 2 }, Coordinates::Space(REC2020_D65)),))
}
#[drip::node(id="test-proof-source", name="display", contract=source_contract)]
pub fn display(_: &KernelContext<'_>, _: &(), mut image: Write<'_, Rgb>) -> drip::Result<()> {
    image.host().copy_from_slice(&PIXELS);
    Ok(())
}

/// Blocks preparation while `block` is set until the test releases it.
pub static SLOW_GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
pub struct SlowKernel {
    #[param(default = false)]
    block: bool,
}
#[drip::node(id="test-slow", name="slow", contract=empty_contract)]
pub fn slow(_: &KernelContext<'_>, _: &SlowKernel) -> drip::Result<()> {
    Ok(())
}
impl GuiNode for slow::Definition {
    type Presentation = PreviewImage;
    const PREPARE: Option<Prepare<Self>> = Some(Prepare::Inputs(|node, _, _| {
        if node.parameters.block {
            let gate = SLOW_GATE.lock().unwrap();
            let (started, release) = gate.as_ref().unwrap();
            started.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Ok(PreviewImage::new(&Arc::new(HostImage {
            width: 1,
            height: 1,
            plane: Placement::whole([1.0, 1.0]),
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: vec![[0.5; 3]].into(),
        })))
    }));
}

#[drip::node(id="test-declared_view", name="Declared view", contract=empty_contract)]
pub fn declared_view(_: &KernelContext<'_>, _: &()) -> drip::Result<()> {
    Ok(())
}
impl GuiNode for declared_view::Definition {
    type Presentation = PreviewImage;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Inputs(|_, _, _| Err(drip::Error::Contract("no view yet".into()))));
}

#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
pub struct CustomSettings {
    #[param(default = false)]
    enabled: bool,
}
#[drip::node(id="test-custom_controls_view", name="Custom controls view", contract=empty_contract)]
pub fn custom_controls_view(_: &KernelContext<'_>, _: &CustomSettings) -> drip::Result<()> {
    Ok(())
}
impl GuiNode for custom_controls_view::Definition {
    type Presentation = PreviewImage;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Inputs(|_, _, _| Err(drip::Error::Contract("no view yet".into()))));
    fn controls(ui: &mut egui::Ui, cx: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        cx.schema(ui);
        ui.label("Custom view controls");
        ui.label(format!("Typed enabled: {}", cx.parameters().enabled));
    }
}

/// Counts preparations; fails them while `fail` is set.
pub static PROBE_CALLS: AtomicUsize = AtomicUsize::new(0);
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
pub struct ProbeSettings {
    #[param(default = true)]
    fail: bool,
}
#[drip::node(id="test-preparation_cache", name="Preparation", contract=empty_contract)]
pub fn probe(_: &KernelContext<'_>, _: &ProbeSettings) -> drip::Result<()> {
    Ok(())
}
impl GuiNode for probe::Definition {
    type Presentation = ();
    const PREPARE: Option<Prepare<Self>> = Some(Prepare::Inputs(|node, _, _| {
        PROBE_CALLS.fetch_add(1, Ordering::Relaxed);
        if node.parameters.fail { Err("preparation failed".into()) } else { Ok(()) }
    }));
}

#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
pub struct WriteKernel {
    #[param(save = [])]
    path: Option<std::path::PathBuf>,
}
fn sink_contract(
    _: &drip::runtime::GlobalContext,
    _: &WriteKernel,
    _: &RgbDesc,
) -> drip::Result<()> {
    Ok(())
}
/// A frontend action that writes the first input pixel at full detail.
#[drip::node(id="test-write", name="write", contract=sink_contract)]
pub fn write(_: &KernelContext<'_>, _: &WriteKernel, _image: Read<'_, Rgb>) -> drip::Result<()> {
    Ok(())
}
impl GuiNode for write::Definition {
    type Presentation = ();
    const ACTION: Option<Action<Self>> = Some(Action {
        name: "write",
        run: |node, inputs, ctx| {
            assert_eq!(ctx.global.scale, 1);
            let image = inputs.image.data.host()?;
            std::fs::write(node.parameters.path.as_ref().unwrap(), format!("{:?}", image[0]))
                .map_err(|e| drip::Error::Runtime(e.to_string()))?;
            Ok(None)
        },
    });
}
