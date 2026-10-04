//! Toy node kinds for exercising the graph engine without real image processing.
#![allow(dead_code)]

use std::sync::Arc;

use drip::eval::{Evaluator, NodeResult};
use drip::graph::{NodeId, Port};
use drip::node::{Action, EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec, Registry};
use drip::param::{ParamKind, ParamSpec};
use drip::project::Project;
use drip::value::{PortType, Rgb, Value, View};

const SCENE: &[PortType] = &[PortType::SceneRec2020];
const DISPLAY: &[PortType] = &[PortType::DisplayRec2020];

pub fn scene(pixel: [f32; 3]) -> Value {
    Value::SceneRec2020(Arc::new(Rgb { width: 1, height: 1, pixels: vec![pixel] }))
}

pub fn pixel(value: &Value) -> [f32; 3] {
    value.rgb().pixels[0]
}

/// Outputs `[value, ctx.scale, 0]`, so tests can see the scale it ran at.
pub static CONST: NodeKind = NodeKind {
    name: "test.const",
    version: 1,
    params: &[ParamSpec {
        name: "value",
        kind: ParamKind::Float { min: -10.0, max: 10.0, default: 1.0 },
    }],
    inputs: &[],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: |p, _, ctx| {
        Ok(Evaluated {
            outputs: vec![scene([p.float("value") as f32, ctx.scale() as f32, 0.0])],
            view: None,
        })
    },
    actions: &[],
};

pub static ADD: NodeKind = NodeKind {
    name: "test.add",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "a", accepts: SCENE }, InputSpec { name: "b", accepts: SCENE }],
    outputs: &[OutputSpec { name: "sum", ty: PortType::SceneRec2020 }],
    eval: |_, inputs, _| {
        let (a, b) = (pixel(&inputs[0]), pixel(&inputs[1]));
        Ok(Evaluated { outputs: vec![scene([a[0] + b[0], a[1] + b[1], a[2] + b[2]])], view: None })
    },
    actions: &[],
};

/// Identity, but changes the semantic type from scene- to display-referred.
pub static TONEMAP: NodeKind = NodeKind {
    name: "test.tonemap",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "scene", accepts: SCENE }],
    outputs: &[OutputSpec { name: "display", ty: PortType::DisplayRec2020 }],
    eval: |_, inputs, _| {
        Ok(Evaluated { outputs: vec![Value::DisplayRec2020(inputs[0].rgb().clone())], view: None })
    },
    actions: &[],
};

pub static FAIL: NodeKind = NodeKind {
    name: "test.fail",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "image", accepts: SCENE }],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: |_, _, _| Err("boom".into()),
    actions: &[],
};

/// A UI-only node: no outputs, presents its input.
pub static VIEW: NodeKind = NodeKind {
    name: "test.view",
    version: 1,
    params: &[],
    inputs: &[InputSpec {
        name: "image",
        accepts: &[PortType::SceneRec2020, PortType::DisplayRec2020],
    }],
    outputs: &[],
    eval: |_, inputs, _| {
        Ok(Evaluated { outputs: vec![], view: Some(View::Image(inputs[0].clone())) })
    },
    actions: &[],
};

/// A sink whose `write` action stores its input pixel at `path`.
pub static WRITE: NodeKind = NodeKind {
    name: "test.write",
    version: 1,
    params: &[ParamSpec { name: "path", kind: ParamKind::Path }],
    inputs: &[InputSpec { name: "image", accepts: DISPLAY }],
    outputs: &[],
    eval: |_, _, _| Ok(Evaluated::default()),
    actions: &[Action {
        name: "write",
        run: |p, inputs| {
            let path = p.path("path").ok_or("no path set")?;
            std::fs::write(path, format!("{:?}", pixel(&inputs[0]))).map_err(|e| e.to_string())
        },
    }],
};

pub static GAIN: NodeKind = NodeKind {
    name: "test.gain",
    version: 1,
    params: &[ParamSpec {
        name: "gain",
        kind: ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 },
    }],
    inputs: &[InputSpec { name: "image", accepts: SCENE }],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: |p, inputs, _| {
        let g = p.float("gain") as f32;
        Ok(Evaluated { outputs: vec![scene(pixel(&inputs[0]).map(|c| c * g))], view: None })
    },
    actions: &[],
};

pub fn registry() -> Registry {
    [&CONST, &ADD, &TONEMAP, &FAIL, &VIEW, &WRITE, &GAIN]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}

pub fn port(id: NodeId, name: &str) -> Port {
    Port(id, name.into())
}

pub const PREVIEW: EvalContext = EvalContext::downscaled(2);

/// Evaluates `id` at preview scale and returns its result.
pub fn eval<'a>(evaluator: &'a mut Evaluator, project: &Project, id: NodeId) -> &'a NodeResult {
    evaluator.evaluate(project, &registry(), PREVIEW, &[id]);
    evaluator.result(id).unwrap()
}

pub fn output(result: &NodeResult) -> [f32; 3] {
    pixel(&result.as_ref().unwrap().outputs[0])
}
