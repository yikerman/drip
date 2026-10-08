use drip::{
    compute::Compute,
    eval::{Evaluation, Evaluator, Request},
    graph::{NodeId, Port},
    image::{ColorImage, Gpu, Rgb},
    project::Project,
};
use std::sync::Arc;
pub const NAME: &str = "wgpu";
pub struct Runner {
    compute: Arc<Compute>,
    evaluator: Evaluator,
    last: Option<Evaluation>,
    host: Option<Arc<Rgb>>,
}
impl Runner {
    pub fn new() -> Self {
        let compute = Compute::new().unwrap();
        Self {
            evaluator: Evaluator::with_compute(compute.clone()),
            compute,
            last: None,
            host: None,
        }
    }
    pub fn run(&mut self, p: &Project, level: u8, target: NodeId, _: bool, host: bool) -> usize {
        self.last = None;
        self.host = None;
        let report = self.evaluator.request(
            &p.graph,
            level,
            &[Request::Output(Port(target, "image".into()))],
        );
        let image = report
            .output(&Port(target, "image".into()))
            .unwrap()
            .downcast::<ColorImage<Gpu>>()
            .unwrap();
        if host {
            self.host = Some(image.download(&self.compute).unwrap().rgb().clone());
        } else {
            self.compute.finish().unwrap();
        }
        let executed = report.executed.len();
        self.last = Some(report);
        executed
    }
    pub fn output(&self, target: NodeId) -> Arc<Rgb> {
        self.host.clone().unwrap_or_else(|| {
            self.last
                .as_ref()
                .unwrap()
                .output(&Port(target, "image".into()))
                .unwrap()
                .downcast_ref::<ColorImage<Gpu>>()
                .unwrap()
                .download(&self.compute)
                .unwrap()
                .rgb()
                .clone()
        })
    }
}
