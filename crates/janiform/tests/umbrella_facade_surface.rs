use janiform::{
    affine_oracle, benchmarking, calendar, calibration, complex_math, hawkes, hourly_factors, news,
    nig, pricing, randomness, return_decomposition, risk_neutral, seasonality, simulation,
    sufficient_statistics, verification,
};

#[test]
fn janiform_keeps_the_curated_top_level_module_roots() {
    let module_roots = collect_public_module_roots(include_str!("../src/lib.rs"));

    assert_eq!(
        module_roots,
        vec![
            "affine_oracle",
            "benchmarking",
            "calendar",
            "calibration",
            "complex_math",
            "hawkes",
            "hourly_factors",
            "news",
            "nig",
            "pricing",
            "randomness",
            "return_decomposition",
            "risk_neutral",
            "seasonality",
            "simulation",
            "sufficient_statistics",
            "verification",
        ]
    );
}

#[test]
fn janiform_exposes_representative_facade_imports_used_during_migration() {
    let _ = std::mem::size_of::<affine_oracle::ExponentialAffineTransformEquationSpecification>();
    let _ = std::mem::size_of::<benchmarking::OraclePricerSufficientStatisticsBenchmarkPolicy>();
    let _ = std::mem::size_of::<calendar::ExchangeCalendarRegistry>();
    let _ = std::mem::size_of::<calendar::TimestampEventPreprocessingOutput>();
    let _ = std::mem::size_of::<calibration::CalibrationStagedExecutionInput>();
    let _ = std::mem::size_of::<calibration::CalibrationStagedRunReport>();
    let _ = std::mem::size_of::<complex_math::ComplexScalar>();
    let _ = std::mem::size_of::<hawkes::HawkesKernelParameters>();
    let _ = std::mem::size_of::<hourly_factors::HourlyFactorProviderRequest<'static>>();
    let _ = std::mem::size_of::<news::NewsProcessConfiguration>();
    let _ = std::mem::size_of::<nig::NigSharedPlusShiftConfiguration>();
    let _ = std::mem::size_of::<pricing::VanillaOptionPricingExecutionInput>();
    let _ = std::mem::size_of::<randomness::SplitMix64SimulationRandomnessStream>();
    let _ = std::mem::size_of::<return_decomposition::ReturnDecompositionInput>();
    let _ = std::mem::size_of::<sufficient_statistics::SufficientStatisticsRunRequest>();
    let _ = std::mem::size_of::<risk_neutral::NigEsscherTransformationInput>();
    let _ = std::mem::size_of::<seasonality::IntradaySeasonalityEstimatorSettings>();
    let _ = std::mem::size_of::<simulation::CoarseSimulationRunRequest>();
    let _ =
        std::mem::size_of::<verification::StylizedFactHorizonComparisonReportGeneratorRequest>();

    let _ = benchmarking::run_oracle_pricer_sufficient_statistics_benchmark_harness;
    let _ = calibration::preprocess_market_data_translation_request;
    let _ = hourly_factors::provide_hourly_factor_contract;
    let _ = pricing::run_vanilla_option_pricing;
    let _ = sufficient_statistics::solve_sufficient_statistics_transform_slices;
    let _ = seasonality::estimate_intraday_seasonality;
    let _ = simulation::run_coarse_simulation;
    let _ = simulation::run_seeded_tick_event_simulation;
    let _ = verification::run_deterministic_regression_harness;
    let _ = verification::run_stylized_fact_horizon_comparison_report_generator;
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
