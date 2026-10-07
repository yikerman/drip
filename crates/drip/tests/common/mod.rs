//! Toy node kinds for exercising the graph engine without real image processing.
#![allow(dead_code)]

use bevy_reflect::Reflect;
use drip::image::{
    Colorimetry, LinearRgb, LinearRgbColorSpace, Linearity, RealMat, Rec2020, Rec2020Mat,
    Rec2020Rgb,
};

// A distinct test-only nominal interpretation with the same RGB capabilities.
#[drip::interpretation(Rec2020Rgb)]
#[derive(Debug, Default, Reflect)]
pub struct TaggedRec2020;
impl Linearity for TaggedRec2020 {}
impl Colorimetry for TaggedRec2020 {
    fn to_xyz_d65(&self, v: [f32; 3]) -> [f32; 3] {
        Rec2020.to_xyz_d65(v)
    }
}
impl LinearRgb for TaggedRec2020 {
    fn color_space(&self) -> &LinearRgbColorSpace {
        Rec2020.color_space()
    }
}
impl Rec2020Rgb for TaggedRec2020 {}
pub type TaggedMat = RealMat<3, TaggedRec2020>;

use drip::node::{KernelError, NodeDeclaration, TypedAction};

use drip::ports::{Read, ReadMat};
use std::sync::Arc;

use drip::eval::{Evaluator, NodeResult};
use drip::graph::{NodeId, Port};
use drip::image::Rgb;
use drip::node::{NodeKind, Registry};
use drip::param::ParamKind;
use drip::project::Project;
use drip::value::Value;

pub fn scene(pixel: [f32; 3]) -> Arc<Rec2020Mat> {
    Arc::new(Rec2020Mat::from(Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![pixel] })))
}

pub fn pixel(value: &Value) -> [f32; 3] {
    value.borrow::<ReadMat<3, dyn Linearity>>().unwrap().rgb().pixels[0]
}

/// Outputs `[value, ctx.scale, 0]`, so tests can see the scale it ran at.
pub static CONST: NodeKind =
    NodeKind::new::<ConstKernel>("test.const", "test", "const", &[], &["image"]);
#[derive(drip::Parameters)]
struct ConstKernel {
    #[param(ParamKind::Float { min: -10.0, max: 10.0, default: 1.0 })]
    value: f32,
}
impl NodeDeclaration for ConstKernel {
    type Parameters = Self;
    type Inputs = ();
    type Outputs = (Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> =
        Some(|p, (), ctx| Ok((scene([p.value, ctx.scale() as f32, 0.0]),)));
}

pub static ADD: NodeKind =
    NodeKind::new::<AddKernel>("test.add", "test", "add", &["a", "b"], &["sum"]);
struct AddKernel;
impl NodeDeclaration for AddKernel {
    type Parameters = ();
    type Inputs = (Read<Rec2020Mat>, Read<Rec2020Mat>);
    type Outputs = (Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|_, (input0, input1), _| {
        let (a, b) = (input0.rgb().pixels[0], input1.rgb().pixels[0]);
        Ok((scene([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),))
    });
}

/// Identity producing a distinct nominal interpretation for exact-port tests.
pub static TONEMAP: NodeKind =
    NodeKind::new::<TonemapKernel>("test.tonemap", "test", "tonemap", &["scene"], &["display"]);
struct TonemapKernel;
impl NodeDeclaration for TonemapKernel {
    type Parameters = ();
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<TaggedMat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> =
        Some(|_, (input0,), _| Ok((Arc::new(TaggedMat::from(input0.rgb().clone())),)));
}

pub static FAIL: NodeKind =
    NodeKind::new::<FailKernel>("test.fail", "test", "fail", &["image"], &["image"]);
struct FailKernel;
impl NodeDeclaration for FailKernel {
    type Parameters = ();
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|_, _, _| Err("boom".into()));
}

/// A UI-only node: no outputs, presents its input.
pub static VIEW: drip::node::TypedNode<ViewKernel> =
    drip::node::TypedNode::new("test.view", "test", "view", &["image"], &[]);
pub struct ViewKernel;
impl NodeDeclaration for ViewKernel {
    type Parameters = ();
    type Inputs = (ReadMat<3, dyn Rec2020Rgb>,);
    type Outputs = ();
}

/// A sink whose `write` action stores its input pixel at `path`.
pub static WRITE: NodeKind =
    NodeKind::new::<WriteKernel>("test.write", "test", "write", &["image"], &[]);
#[derive(drip::Parameters)]
struct WriteKernel {
    #[param(ParamKind::Path { output: true })]
    #[external]
    path: Option<std::path::PathBuf>,
}
impl NodeDeclaration for WriteKernel {
    type Parameters = Self;
    type Inputs = (Read<TaggedMat>,);
    type Outputs = ();
    const ACTIONS: &'static [TypedAction<Self>] = &[TypedAction {
        name: "write",
        run: |p, (input0,), _| {
            let path = p.path.as_deref().ok_or("no path set")?;
            std::fs::write(path, format!("{:?}", input0.rgb().pixels[0]))
                .map_err(|e| KernelError::Failed(e.to_string()))
        },
    }];
}

/// Multiplies its input by `gain`.
pub static GAIN: NodeKind =
    NodeKind::new::<GainKernel>("test.gain", "test", "gain", &["image"], &["image"]);
#[derive(drip::Parameters)]
struct GainKernel {
    #[param(ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 })]
    gain: f32,
}
impl NodeDeclaration for GainKernel {
    type Parameters = Self;
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|p, (input0,), _| {
        let g = p.gain;
        Ok((scene(input0.rgb().pixels[0].map(|c| c * g)),))
    });
}

/// Outputs the length of the file at `path`, read through the resource store.
pub static FILE: NodeKind =
    NodeKind::new::<FileKernel>("test.file", "test", "file", &[], &["image"]);
#[derive(drip::Parameters)]
struct FileKernel {
    #[param(ParamKind::Path { output: false })]
    path: Option<std::path::PathBuf>,
}
impl NodeDeclaration for FileKernel {
    type Parameters = Self;
    type Inputs = ();
    type Outputs = (Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|p, (), ctx| {
        let len = ctx.resources().load(p.path.as_deref().ok_or("no path set")?, |path| {
            std::fs::read(path).map(|bytes| bytes.len()).map_err(|e| e.to_string())
        })?;
        Ok((scene([*len as f32, 0.0, 0.0]),))
    });
}

pub fn registry() -> Registry {
    [&CONST, &ADD, &TONEMAP, &FAIL, VIEW.kind(), &WRITE, &GAIN, &FILE]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}

pub fn port(id: NodeId, name: &str) -> Port {
    Port(id, name.into())
}

/// Downscale level for interactive-style evaluation: a quarter of full size.
pub const PREVIEW: u8 = 2;

/// Evaluates `id` at preview scale and returns its result.
pub fn eval<'a>(evaluator: &'a mut Evaluator, project: &Project, id: NodeId) -> &'a NodeResult {
    evaluator.evaluate(&project.graph, PREVIEW, &[id]);
    evaluator.result(id).unwrap()
}

pub fn output(result: &NodeResult) -> [f32; 3] {
    pixel(&result.as_ref().unwrap()[0])
}
