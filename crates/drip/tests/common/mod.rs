//! Toy node kinds for exercising the graph engine without real image processing.
#![allow(dead_code)]

use bevy_reflect::Reflect;
use drip::image::{
    Colorimetry, LinearRgb, LinearRgbColorSpace, Linearity, RealMat, Rec2020, Rec2020Mat,
    Rec2020Rgb,
};
use drip::ports::MatRef;

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

use drip::node::{EvalContext, KernelError, NodeKernel, TypedAction};

use drip::ports::{Read, ReadMat};
use drip::view::PreviewImage;
use std::sync::Arc;

use drip::eval::{Evaluator, NodeResult};
use drip::graph::{NodeId, Port};
use drip::image::Rgb;
use drip::node::{Evaluated, NodeKind, Registry};
use drip::param::ParamKind;
use drip::project::Project;
use drip::value::Value;
use drip::view::View;

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
impl NodeKernel for ConstKernel {
    type Parameters = Self;
    type View = ();
    type Inputs = ();
    type Outputs = (Arc<Rec2020Mat>,);

    fn eval(
        p: Self::Parameters,
        (): (),
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        Ok(Evaluated { outputs: (scene([p.value, ctx.scale() as f32, 0.0]),), view: () })
    }
}

pub static ADD: NodeKind =
    NodeKind::new::<AddKernel>("test.add", "test", "add", &["a", "b"], &["sum"]);
struct AddKernel;
impl NodeKernel for AddKernel {
    type Parameters = ();
    type View = ();
    type Inputs = (Read<Rec2020Mat>, Read<Rec2020Mat>);
    type Outputs = (Arc<Rec2020Mat>,);

    fn eval(
        _: Self::Parameters,
        (input0, input1): (&Rec2020Mat, &Rec2020Mat),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        let (a, b) = (input0.rgb().pixels[0], input1.rgb().pixels[0]);
        Ok(Evaluated { outputs: (scene([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),), view: () })
    }
}

/// Identity producing a distinct nominal interpretation for exact-port tests.
pub static TONEMAP: NodeKind =
    NodeKind::new::<TonemapKernel>("test.tonemap", "test", "tonemap", &["scene"], &["display"]);
struct TonemapKernel;
impl NodeKernel for TonemapKernel {
    type Parameters = ();
    type View = ();
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<TaggedMat>,);

    fn eval(
        _: Self::Parameters,
        (input0,): (&Rec2020Mat,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        Ok(Evaluated { outputs: (Arc::new(TaggedMat::from(input0.rgb().clone())),), view: () })
    }
}

pub static FAIL: NodeKind =
    NodeKind::new::<FailKernel>("test.fail", "test", "fail", &["image"], &["image"]);
struct FailKernel;
impl NodeKernel for FailKernel {
    type Parameters = ();
    type View = ();
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<Rec2020Mat>,);

    fn eval(
        _: Self::Parameters,
        _: (&Rec2020Mat,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        Err("boom".into())
    }
}

/// A UI-only node: no outputs, presents its input.
pub static VIEW: NodeKind =
    NodeKind::new::<ViewKernel>("test.view", "test", "view", &["image"], &[]);
struct ViewKernel;
impl NodeKernel for ViewKernel {
    type Parameters = ();
    type View = Option<View>;
    type Inputs = (ReadMat<3, dyn Rec2020Rgb>,);
    type Outputs = ();

    fn eval(
        _: Self::Parameters,
        (input0,): (MatRef<'_, 3, dyn Rec2020Rgb>,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        Ok(Evaluated { outputs: (), view: Some(View::Image(PreviewImage::from_input(&input0))) })
    }
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
impl NodeKernel for WriteKernel {
    type Parameters = Self;
    type View = ();
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
    fn eval(
        _: Self::Parameters,
        _: (&TaggedMat,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        Ok(Evaluated::default())
    }
}

/// Multiplies its input by `gain`.
pub static GAIN: NodeKind =
    NodeKind::new::<GainKernel>("test.gain", "test", "gain", &["image"], &["image"]);
#[derive(drip::Parameters)]
struct GainKernel {
    #[param(ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 })]
    gain: f32,
}
impl NodeKernel for GainKernel {
    type Parameters = Self;
    type View = ();
    type Inputs = (Read<Rec2020Mat>,);
    type Outputs = (Arc<Rec2020Mat>,);

    fn eval(
        p: Self::Parameters,
        (input0,): (&Rec2020Mat,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        let g = p.gain;
        Ok(Evaluated { outputs: (scene(input0.rgb().pixels[0].map(|c| c * g)),), view: () })
    }
}

/// Outputs the length of the file at `path`, read through the resource store.
pub static FILE: NodeKind =
    NodeKind::new::<FileKernel>("test.file", "test", "file", &[], &["image"]);
#[derive(drip::Parameters)]
struct FileKernel {
    #[param(ParamKind::Path { output: false })]
    path: Option<std::path::PathBuf>,
}
impl NodeKernel for FileKernel {
    type Parameters = Self;
    type View = ();
    type Inputs = ();
    type Outputs = (Arc<Rec2020Mat>,);

    fn eval(
        p: Self::Parameters,
        (): (),
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError> {
        let len = ctx.resources().load(p.path.as_deref().ok_or("no path set")?, |path| {
            std::fs::read(path).map(|bytes| bytes.len()).map_err(|e| e.to_string())
        })?;
        Ok(Evaluated { outputs: (scene([*len as f32, 0.0, 0.0]),), view: () })
    }
}

pub fn registry() -> Registry {
    [&CONST, &ADD, &TONEMAP, &FAIL, &VIEW, &WRITE, &GAIN, &FILE]
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
    pixel(&result.as_ref().unwrap().outputs[0])
}
