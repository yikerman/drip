// Included in worker.rs only while compiling this benchmark. It uses the actual
// worker's evaluation and view preparation, including all three CPU scopes.
use super::*;
#[test]
#[ignore = "manual sequential performance measurement"]
fn compare_actual_worker() {
    let raw = std::env::var("DRIP_BENCH_RAW").unwrap();
    let level: u8 = std::env::var("DRIP_BENCH_LEVEL").unwrap().parse().unwrap();
    let repetitions: usize = std::env::var("DRIP_BENCH_REPETITIONS").unwrap().parse().unwrap();
    let mut project = drip::templates::raw_to_tiff();
    project
        .graph
        .set_param(project.graph.find("RAW").unwrap(), "path", serde_json::json!(raw))
        .unwrap();
    let targets: Vec<_> = project.graph.nodes().map(|(id, _)| id).collect();
    let mut runner = BenchWorker::new();
    let mut previous = None;
    for scenario in ["highlight", "exposure", "sigmoid", "noop"] {
        for (name, key, value) in [
            ("Highlights", "threshold", 0.98),
            ("Exposure", "ev", 0.0),
            ("Sigmoid", "contrast", 1.5),
        ] {
            project
                .graph
                .set_param(project.graph.find(name).unwrap(), key, serde_json::json!(value))
                .unwrap();
        }
        for _ in 0..2 {
            let req = Request {
                generation: 0,
                graph: project.graph.clone(),
                level,
                targets: targets.clone(),
            };
            previous = Some(runner.run(&req));
            runner.collect();
        }
        for i in 0..repetitions {
            let edit = match scenario {
                "highlight" => {
                    Some(("Highlights", "threshold", if i % 2 == 0 { 0.97 } else { 0.98 }))
                }
                "exposure" => Some(("Exposure", "ev", if i % 2 == 0 { 0.25 } else { 0.5 })),
                "sigmoid" => Some(("Sigmoid", "contrast", if i % 2 == 0 { 1.4 } else { 1.6 })),
                _ => None,
            };
            if let Some((name, key, value)) = edit {
                project
                    .graph
                    .set_param(project.graph.find(name).unwrap(), key, serde_json::json!(value))
                    .unwrap();
            }
            let req = Request {
                generation: i as u64,
                graph: project.graph.clone(),
                level,
                targets: targets.clone(),
            };
            let start = Instant::now();
            let event = runner.run(&req);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            if let Event::Evaluated { views, failures, .. } = &event {
                assert!(failures.is_empty(), "worker failed");
                assert_eq!(
                    views.values().filter(|v| v.as_ref().is_ok_and(Option::is_some)).count(),
                    4
                );
            } else {
                panic!("unexpected event");
            }
            println!("BENCH,{},{scenario},{level},{i},{ms:.6}", BenchWorker::NAME);
            previous = Some(event);
            runner.collect();
        }
    }
    drop(previous);
}
