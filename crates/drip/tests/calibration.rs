use drip::{eval::Evaluator, graph::Dag, node, node::data::*, ports::*, runtime::RuntimeContext};

fn metadata(phase: BayerPhase) -> CaptureData {
    CaptureData {
        bayer: Some(BayerCalibration {
            phase,
            source: Camera { coordinates: "sensor".into(), scale: "relative".into() },
            levels: [0.5, 1., 1.5, 2.],
            gains: Some(SiteGains {
                target: Camera { coordinates: "balanced".into(), scale: "relative".into() },
                values: [2., 3., 4., 5.],
            }),
        }),
        ..Default::default()
    }
}

fn combined_gains(runtime: RuntimeContext) {
    let eval = Evaluator::new(runtime);
    for phase in [BayerPhase::Rggb, BayerPhase::Grbg, BayerPhase::Gbrg, BayerPhase::Bggr] {
        let mut dag = Dag::new();
        let data = metadata(phase);
        let calibration = data.bayer.as_ref().unwrap();
        let desc = BayerDesc {
            extent: Extent { width: 3, height: 3 },
            phase,
            interpretation: calibration.source.clone(),
        };
        let source =
            node::source::<Bayer>(&mut dag, desc, vec![-1., 0., 0.5, 1., 2., 3., 4., 5., 6.])
                .unwrap();
        let metadata =
            node::source::<CaptureMetadata>(&mut dag, MetadataDesc::of(&data), data).unwrap();
        let gains = node::bayer_gains::add(&mut dag, ()).unwrap();
        dag.connect(source, gains.image).unwrap();
        dag.connect(metadata, gains.metadata).unwrap();
        let image = eval.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), gains.output).unwrap();
        // Local sites repeat every two columns and rows, including partial cells.
        assert_eq!(image.data.as_ref().as_ref(), &[-2., 0., 1., 4., 10., 12., 8., 15., 12.]);
        let levels =
            eval.evaluate::<Cpu<BayerLevels>>(&dag, &Default::default(), gains.levels).unwrap();
        assert_eq!(*levels.data, [1., 3., 6., 10.]);
        assert_eq!(levels.desc.phase, phase);
        assert_eq!(levels.desc.interpretation, image.desc.interpretation);
        assert_eq!(image.desc.interpretation.coordinates, "balanced");
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
fn contracts_reject_mismatched_sites_and_missing_gains_stay_unevaluable() {
    let mut dag = Dag::new();
    let mut data = metadata(BayerPhase::Rggb);
    let desc = BayerDesc {
        extent: Extent { width: 1, height: 1 },
        phase: BayerPhase::Grbg,
        interpretation: data.bayer.as_ref().unwrap().source.clone(),
    };
    let source = node::source::<Bayer>(&mut dag, desc, vec![1.]).unwrap();
    let gains = node::bayer_gains::add(&mut dag, ()).unwrap();
    dag.connect(source, gains.image).unwrap();
    let wrong =
        node::source::<CaptureMetadata>(&mut dag, MetadataDesc::of(&data), data.clone()).unwrap();
    assert!(dag.connect(wrong, gains.metadata).is_err());
    data.bayer.as_mut().unwrap().phase = BayerPhase::Grbg;
    data.bayer.as_mut().unwrap().gains = None;
    let missing = node::source::<CaptureMetadata>(&mut dag, MetadataDesc::of(&data), data).unwrap();
    dag.connect(missing, gains.metadata).unwrap();
    assert!(dag.description(gains.output).unwrap().is_none());
    assert!(dag.description(gains.levels).unwrap().is_none());
    let eval = Evaluator::new(RuntimeContext::host());
    assert!(eval.evaluate::<Cpu<BayerLevels>>(&dag, &Default::default(), gains.levels).is_err());
    assert!(eval.evaluate::<Cpu<CaptureMetadata>>(&dag, &Default::default(), missing).is_ok());
}
