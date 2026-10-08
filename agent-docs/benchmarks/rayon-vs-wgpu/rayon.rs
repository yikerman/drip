use drip::{
    eval::Evaluator,
    graph::NodeId,
    image::{Rec2020Mat, Rgb},
    project::Project,
};
use std::sync::Arc;
pub const NAME: &str = "rayon";
pub struct Runner {
    evaluator: Evaluator,
}
impl Runner {
    pub fn new() -> Self {
        rayon::ThreadPoolBuilder::new().build_global().unwrap();
        eprintln!("rayon_threads={}", rayon::current_num_threads());
        Self { evaluator: Evaluator::default() }
    }
    pub fn run(&mut self, p: &Project, level: u8, target: NodeId, cached: bool, _: bool) -> usize {
        if !cached {
            self.evaluator = self.evaluator.fork();
        }
        let executed = self.evaluator.evaluate(&p.graph, level, &[target]);
        self.evaluator.result(target).unwrap().as_ref().unwrap();
        executed.len()
    }
    pub fn output(&self, target: NodeId) -> Arc<Rgb> {
        self.evaluator.result(target).unwrap().as_ref().unwrap()[0]
            .downcast_ref::<Rec2020Mat>()
            .unwrap()
            .rgb()
            .clone()
    }
}
