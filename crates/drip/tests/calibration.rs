use drip::{eval::Evaluator, graph::Dag, node, node::data::*, ports::*, runtime::RuntimeContext};
use std::sync::Arc;

/// As-shot R, G, B and second-G gains 2, 1, 4, 3; the second green's black is
/// half the white range, so its clipping level is 0.5 before gains.
fn metadata(cfa: [[u8; 2]; 2]) -> Arc<drip_raw::Metadata> {
    let mut m =
        drip_raw::Metadata { make: "Test".into(), model: "Camera".into(), ..Default::default() };
    m.calibration.cfa = cfa;
    m.calibration.maximum = 1000;
    m.calibration.channel_black = [0, 0, 0, 500];
    m.calibration.as_shot = [2., 1., 4., 3.];
    Arc::new(m)
}
fn response(m: &drip_raw::Metadata) -> Response {
    Response { camera: Camera::of(m), balance: None }
}
/// Phase, LibRaw CFA, expected site gains and balanced clipping levels.
type Case = (BayerPhase, [[u8; 2]; 2], [f32; 4], [f32; 4]);
const CASES: [Case; 4] = [
    (BayerPhase::Rggb, [[0, 1], [3, 2]], [2., 1., 3., 4.], [2., 1., 1.5, 4.]),
    (BayerPhase::Grbg, [[1, 0], [2, 3]], [1., 2., 4., 3.], [1., 2., 4., 1.5]),
    (BayerPhase::Gbrg, [[1, 2], [0, 3]], [1., 4., 2., 3.], [1., 4., 2., 1.5]),
    (BayerPhase::Bggr, [[2, 1], [3, 0]], [4., 1., 3., 2.], [4., 1., 1.5, 2.]),
];

fn combined_gains(runtime: RuntimeContext) {
    let eval = Evaluator::new(runtime);
    for (phase, cfa, gains, levels) in CASES {
        let mut dag = Dag::new();
        let m = metadata(cfa);
        let desc =
            BayerDesc { extent: Extent { width: 3, height: 3 }, phase, response: response(&m) };
        let samples = [-1., 0., 0.5, 1., 2., 3., 4., 5., 6.];
        let source = node::source::<Bayer>(&mut dag, desc, samples.to_vec()).unwrap();
        let metadata = node::source::<CaptureMetadata>(&mut dag, m.clone(), m).unwrap();
        let node = node::bayer_gains::add(&mut dag, ()).unwrap();
        dag.connect(source, node.image).unwrap();
        dag.connect(metadata, node.metadata).unwrap();
        let image = eval.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), node.output).unwrap();
        // Local sites repeat every two columns and rows, including partial cells.
        let expected: Vec<f32> =
            samples.iter().enumerate().map(|(i, v)| v * gains[i / 3 % 2 * 2 + i % 3 % 2]).collect();
        assert_eq!(image.data.as_ref().as_ref(), expected.as_slice());
        let result =
            eval.evaluate::<Cpu<BayerLevels>>(&dag, &Default::default(), node.levels).unwrap();
        assert_eq!(*result.data, levels);
        assert_eq!(result.desc.phase, phase);
        assert_eq!(result.desc.response, image.desc.response);
        assert_eq!(image.desc.response.balance, Some([2., 1., 4., 3.]));
        assert_eq!(image.statistics.uploads, 1); // Only the image is a device input.
    }
}

#[cfg(feature = "cpu")]
#[test]
fn cpu_combines_samples_and_clipping_levels() {
    combined_gains(RuntimeContext::cpu());
}

#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_combines_samples_and_clipping_levels() {
    combined_gains(RuntimeContext::from_env().unwrap());
}

#[test]
fn contracts_reject_mismatched_sites_and_missing_gains() {
    let gains = node::bayer_gains::definition(()).unwrap();
    let check = |m: Arc<drip_raw::Metadata>| {
        let desc = BayerDesc {
            extent: Extent { width: 1, height: 1 },
            phase: BayerPhase::Grbg,
            response: response(&m),
        };
        let inputs: [Option<Description>; 2] = [Some(Arc::new(desc)), Some(Arc::new(m))];
        gains.contract(&Default::default(), &inputs)
    };
    assert!(check(metadata([[1, 0], [2, 3]])).is_ok());
    assert!(check(metadata([[0, 1], [3, 2]])).is_err());
    let mut unbalanced = (*metadata([[1, 0], [2, 3]])).clone();
    unbalanced.calibration.as_shot = [0.; 4];
    assert!(check(Arc::new(unbalanced)).is_err());
}
