struct BenchWorker {
    evaluator: Evaluator,
    images: ImageCache,
    presentations: CachedPresentations,
}
impl BenchWorker {
    const NAME: &'static str = "rayon-worker";
    fn new() -> Self {
        Self {
            evaluator: Evaluator::default(),
            images: ImageCache::default(),
            presentations: BTreeMap::new(),
        }
    }
    fn run(&mut self, request: &Request) -> Event {
        evaluate(request, &mut self.evaluator, &mut self.images, &mut self.presentations)
    }
    fn collect(&mut self) {
        self.images.collect();
    }
}
