use janiform::{calibration, foundation, governance, model, pricing, simulation, time, validation};

#[test]
fn janiform_keeps_the_curated_top_level_module_roots() {
    let module_roots = collect_public_module_roots(include_str!("../src/lib.rs"));

    assert_eq!(
        module_roots,
        vec![
            "calibration",
            "foundation",
            "governance",
            "model",
            "pricing",
            "simulation",
            "time",
            "validation",
        ]
    );
}

#[test]
fn janiform_exposes_representative_semantic_facade_imports() {
    let _ = std::mem::size_of::<
        model::affine_oracle::ExponentialAffineTransformEquationSpecification,
    >();
    let _ = std::mem::size_of::<
        governance::benchmarking::OraclePricerSufficientStatisticsBenchmarkPolicy,
    >();
    let _ = std::mem::size_of::<time::calendar::ExchangeCalendarRegistry>();
    let _ = std::mem::size_of::<time::calendar::TimestampEventPreprocessingOutput>();
    let _ = std::mem::size_of::<calibration::staged::CalibrationStagedExecutionInput>();
    let _ = std::mem::size_of::<calibration::staged::CalibrationStagedRunReport>();
    let _ = std::mem::size_of::<foundation::math::ComplexScalar>();
    let _ = std::mem::size_of::<model::hawkes::HawkesKernelParameters>();
    let _ = std::mem::size_of::<model::factors::hourly::HourlyFactorProviderRequest<'static>>();
    let _ = std::mem::size_of::<model::news::NewsProcessConfiguration>();
    let _ = std::mem::size_of::<model::nig::NigSharedPlusShiftConfiguration>();
    let _ = std::mem::size_of::<pricing::VanillaOptionPricingExecutionInput>();
    let _ = std::mem::size_of::<foundation::randomness::SplitMix64SimulationRandomnessStream>();
    let _ = std::mem::size_of::<model::returns::ReturnDecompositionInput>();
    let _ = std::mem::size_of::<model::sufficient_statistics::SufficientStatisticsRunRequest>();
    let _ = std::mem::size_of::<model::risk_neutral::NigEsscherTransformationInput>();
    let _ = std::mem::size_of::<simulation::CoarseSimulationRunRequest>();
    let _ = std::mem::size_of::<
        validation::stylized_facts::StylizedFactHorizonComparisonReportGeneratorRequest,
    >();

    let _ = governance::benchmarking::run_oracle_pricer_sufficient_statistics_benchmark_harness;
    let _ = calibration::market_data::preprocess_market_data_translation_request;
    let _ = model::factors::hourly::provide_hourly_factor_contract;
    let _ = pricing::run_vanilla_option_pricing;
    let _ = model::sufficient_statistics::solve_sufficient_statistics_transform_slices;
    let _ = simulation::run_coarse_simulation;
    let _ = simulation::run_seeded_tick_event_simulation;
    let _ = governance::deterministic_regression::run_deterministic_regression_harness;
    let _ = validation::stylized_facts::run_stylized_fact_horizon_comparison_report_generator;
}

fn collect_public_module_roots(lib_rs_source: &str) -> Vec<&str> {
    lib_rs_source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let module_name = line
                .strip_prefix("pub mod ")?
                .strip_suffix(';')
                .expect("top-level pub mod declaration should end with semicolon");
            Some(module_name)
        })
        .collect()
}
