//! Toy node kinds for exercising the graph engine without real image processing.
#![allow(dead_code)]

use drip::image::{
    DisplayRec2020, LinearThreeChannelMatrix, Rec2020, RgbIn, SceneRec2020, ThreeChannelMatrix,
};
use drip::node::{EvalContext, NodeKernel, TypedAction};
use drip::param::Params;
use drip::ports::Read;
use drip::view::PreviewImage;
use std::sync::Arc;

use drip::eval::{Evaluator, NodeResult};
use drip::graph::{NodeId, Port};
use drip::image::Rgb;
use drip::node::{Evaluated, NodeKind, Registry};
use drip::param::{ParamKind, ParamSpec};
use drip::project::Project;
use drip::value::Value;
use drip::view::View;

pub fn scene(pixel: [f32; 3]) -> Arc<SceneRec2020> {
    Arc::new(SceneRec2020::from(Arc::new(Rgb {
        width: 1,
        height: 1,
        scale: 1,
        pixels: vec![pixel],
    })))
}

pub fn pixel(value: &Value) -> [f32; 3] {
    value.borrow::<Read<dyn LinearThreeChannelMatrix>>().unwrap().rgb().pixels[0]
}

/// Outputs `[value, ctx.scale, 0]`, so tests can see the scale it ran at.
pub static CONST: NodeKind = NodeKind::new::<ConstKernel>(
    "test.const",
    "const",
    &[ParamSpec::new("value", ParamKind::Float { min: -10.0, max: 10.0, default: 1.0 })],
    &[],
    &["image"],
);
struct ConstKernel;
impl NodeKernel for ConstKernel {
    type Inputs = ();
    type Outputs = (Arc<SceneRec2020>,);

    fn eval(
        p: Params<'_>,
        (): (),
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated {
            outputs: (scene([p.float("value") as f32, ctx.scale() as f32, 0.0]),),
            view: None,
        })
    }
}

pub static ADD: NodeKind =
    NodeKind::new::<AddKernel>("test.add", "add", &[], &["a", "b"], &["sum"]);
struct AddKernel;
impl NodeKernel for AddKernel {
    type Inputs = (Read<SceneRec2020>, Read<SceneRec2020>);
    type Outputs = (Arc<SceneRec2020>,);

    fn eval(
        _: Params<'_>,
        (input0, input1): (&SceneRec2020, &SceneRec2020),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let (a, b) = (input0.rgb().pixels[0], input1.rgb().pixels[0]);
        Ok(Evaluated { outputs: (scene([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),), view: None })
    }
}

/// Identity, but changes the semantic type from scene- to display-referred.
pub static TONEMAP: NodeKind =
    NodeKind::new::<TonemapKernel>("test.tonemap", "tonemap", &[], &["scene"], &["display"]);
struct TonemapKernel;
impl NodeKernel for TonemapKernel {
    type Inputs = (Read<SceneRec2020>,);
    type Outputs = (Arc<DisplayRec2020>,);

    fn eval(
        _: Params<'_>,
        (input0,): (&SceneRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated {
            outputs: (Arc::new(DisplayRec2020::from(input0.rgb().clone())),),
            view: None,
        })
    }
}

pub static FAIL: NodeKind =
    NodeKind::new::<FailKernel>("test.fail", "fail", &[], &["image"], &["image"]);
struct FailKernel;
impl NodeKernel for FailKernel {
    type Inputs = (Read<SceneRec2020>,);
    type Outputs = (Arc<SceneRec2020>,);

    fn eval(
        _: Params<'_>,
        _: (&SceneRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Err("boom".into())
    }
}

/// A UI-only node: no outputs, presents its input.
pub static VIEW: NodeKind = NodeKind::new::<ViewKernel>("test.view", "view", &[], &["image"], &[]);
struct ViewKernel;
impl NodeKernel for ViewKernel {
    type Inputs = (Read<dyn RgbIn<Rec2020>>,);
    type Outputs = ();

    fn eval(
        _: Params<'_>,
        (input0,): (&dyn RgbIn<Rec2020>,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated { outputs: (), view: Some(View::Image(PreviewImage::new(input0))) })
    }
}

/// A sink whose `write` action stores its input pixel at `path`.
pub static WRITE: NodeKind = NodeKind::new::<WriteKernel>(
    "test.write",
    "write",
    &[ParamSpec::new("path", ParamKind::Path { output: true }).external()],
    &["image"],
    &[],
);
struct WriteKernel;
impl NodeKernel for WriteKernel {
    type Inputs = (Read<DisplayRec2020>,);
    type Outputs = ();
    const ACTIONS: &'static [TypedAction<Self>] = &[TypedAction {
        name: "write",
        run: |p, (input0,), _| {
            let path = p.path("path").ok_or("no path set")?;
            std::fs::write(path, format!("{:?}", input0.rgb().pixels[0])).map_err(|e| e.to_string())
        },
    }];
    fn eval(
        _: Params<'_>,
        _: (&DisplayRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated::default())
    }
}

/// Multiplies its input by `gain`.
pub static GAIN: NodeKind = NodeKind::new::<GainKernel>(
    "test.gain",
    "gain",
    &[ParamSpec::new("gain", ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 })],
    &["image"],
    &["image"],
);
struct GainKernel;
impl NodeKernel for GainKernel {
    type Inputs = (Read<SceneRec2020>,);
    type Outputs = (Arc<SceneRec2020>,);

    fn eval(
        p: Params<'_>,
        (input0,): (&SceneRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let g = p.float("gain") as f32;
        Ok(Evaluated { outputs: (scene(input0.rgb().pixels[0].map(|c| c * g)),), view: None })
    }
}

/// Outputs the length of the file at `path`, read through the resource store.
pub static FILE: NodeKind = NodeKind::new::<FileKernel>(
    "test.file",
    "file",
    &[ParamSpec::new("path", ParamKind::Path { output: false })],
    &[],
    &["image"],
);
struct FileKernel;
impl NodeKernel for FileKernel {
    type Inputs = ();
    type Outputs = (Arc<SceneRec2020>,);

    fn eval(
        p: Params<'_>,
        (): (),
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let len = ctx.resources().load(p.path("path").ok_or("no path set")?, |path| {
            std::fs::read(path).map(|bytes| bytes.len()).map_err(|e| e.to_string())
        })?;
        Ok(Evaluated { outputs: (scene([*len as f32, 0.0, 0.0]),), view: None })
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
