struct BenchWorker {
    evaluator: Evaluator,
    compute: Arc<Compute>,
    images: ImageCache,
}
impl BenchWorker {
    const NAME: &'static str = "wgpu-worker";
    fn new() -> Self {
        let compute = Compute::new().unwrap();
        Self {
            evaluator: Evaluator::with_compute(compute.clone()),
            compute,
            images: ImageCache::default(),
        }
    }
    fn run(&mut self, request: &Request) -> Event {
        let event = evaluate(request, &self.evaluator, &mut self.images);
        self.compute.finish().unwrap();
        event
    }
    fn collect(&mut self) {
        self.images.collect();
    }
}
