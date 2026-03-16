use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_engine_risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskRunRequest,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_validation::{
    RegressionCaseStatus, SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_COVERAGE_POLICY,
    SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_DETERMINISM_POLICY,
    SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_OUTPUT_SCHEMA_METADATA_VERSION,
    SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_REPORT_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION,
    SufficientStatisticsMultiIntervalValidationError,
    SufficientStatisticsMultiIntervalValidationScenario,
    SufficientStatisticsMultiIntervalValidationThresholds,
    SufficientStatisticsValidationHorizonCategory,
    run_sufficient_statistics_multi_interval_validation_suite,
};

#[test]
fn validation_suite_passes_short_medium_and_long_horizon_scenarios() {
    let scenarios = build_validation_scenarios();

    let report = run_sufficient_statistics_multi_interval_validation_suite(
        &scenarios,
        SufficientStatisticsMultiIntervalValidationThresholds::default(),
    )
    .expect("validation suite should run for short, medium, and long horizon scenarios");

    assert_eq!(
        report.schema_version,
        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.determinism_policy,
        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_DETERMINISM_POLICY
    );
    assert_eq!(
        report.coverage_policy,
        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_COVERAGE_POLICY
    );
    assert_eq!(
        report.output_schema_metadata.schema_version,
        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert_eq!(
        report.output_schema_metadata.thresholds_schema_version,
        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION
    );
    assert_eq!(report.summary.total_case_count, scenarios.len());
    assert_eq!(report.summary.failed_case_count, 0);
    assert_eq!(
        report.summary.passed_case_count,
        report.summary.total_case_count,
    );
    assert_eq!(report.summary.deterministic_failed_check_count, 0);
    assert_eq!(report.summary.statistical_failed_check_count, 0);

    assert!(
        report
            .case_reports
            .iter()
            .all(
                |case_report| case_report.status == RegressionCaseStatus::Pass
                    && case_report.deterministic_checks.len() == 2
                    && case_report.statistical_checks.len() == case_report.interval_count * 4
            )
    );

    let has_short = report.case_reports.iter().any(|case_report| {
        case_report.horizon_category == SufficientStatisticsValidationHorizonCategory::Short
    });
    let has_medium = report.case_reports.iter().any(|case_report| {
        case_report.horizon_category == SufficientStatisticsValidationHorizonCategory::Medium
    });
    let has_long = report.case_reports.iter().any(|case_report| {
        case_report.horizon_category == SufficientStatisticsValidationHorizonCategory::Long
    });
    assert!(has_short && has_medium && has_long);
}

#[test]
fn validation_suite_report_is_repeatable_for_fixed_inputs() {
    let scenarios = build_validation_scenarios();
    let thresholds = SufficientStatisticsMultiIntervalValidationThresholds::default();

    let first_report =
        run_sufficient_statistics_multi_interval_validation_suite(&scenarios, thresholds.clone())
            .expect("first validation suite run should succeed");
    let second_report =
        run_sufficient_statistics_multi_interval_validation_suite(&scenarios, thresholds)
            .expect("second validation suite run should succeed");

    assert_eq!(first_report, second_report);
    assert_eq!(
        serde_json::to_string(&first_report).expect("first report should serialize"),
        serde_json::to_string(&second_report).expect("second report should serialize"),
    );
}

#[test]
fn validation_suite_separates_deterministic_and_statistical_failures() {
    let scenarios = build_validation_scenarios();

    let mut strict_thresholds = SufficientStatisticsMultiIntervalValidationThresholds::default();
    strict_thresholds.statistical_sample_count = 32;
    strict_thresholds.integrated_intensity_mean_absolute_floor = 0.0;
    strict_thresholds.integrated_intensity_mean_relative_tolerance = 0.0;
    strict_thresholds.event_count_mean_absolute_floor = 0.0;
    strict_thresholds.event_count_mean_relative_tolerance = 0.0;

    let report = run_sufficient_statistics_multi_interval_validation_suite(
        &scenarios,
        strict_thresholds,
    )
    .expect("validation suite should produce a deterministic report even with strict thresholds");

    assert_eq!(report.summary.deterministic_failed_check_count, 0);
    assert!(report.summary.statistical_failed_check_count > 0);
    assert!(report.summary.failed_case_count > 0);
    assert!(
        report
            .case_reports
            .iter()
            .any(|case_report| case_report.status == RegressionCaseStatus::Fail)
    );
}

#[test]
fn validation_suite_requires_short_medium_and_long_horizon_coverage() {
    let mut scenarios = build_validation_scenarios();
    scenarios.retain(|scenario| {
        scenario.horizon_category != SufficientStatisticsValidationHorizonCategory::Long
    });

    let error = run_sufficient_statistics_multi_interval_validation_suite(
        &scenarios,
        SufficientStatisticsMultiIntervalValidationThresholds::default(),
    )
    .expect_err("missing long-horizon coverage must fail validation-suite execution");

    match error {
        SufficientStatisticsMultiIntervalValidationError::MissingHorizonCoverage {
            missing_horizon_categories,
        } => {
            assert_eq!(
                missing_horizon_categories,
                vec![SufficientStatisticsValidationHorizonCategory::Long],
            );
        }
        other => panic!("unexpected validation-suite error variant: {other}"),
    }
}

#[test]
fn validation_suite_report_schema_metadata_exposes_required_fields() {
    let scenarios = build_validation_scenarios();

    let report = run_sufficient_statistics_multi_interval_validation_suite(
        &scenarios,
        SufficientStatisticsMultiIntervalValidationThresholds::default(),
    )
    .expect("validation suite run should succeed");

    let serialized_report =
        serde_json::to_value(&report).expect("validation report should serialize to JSON value");
    let output_schema_metadata = serialized_report
        .get("output_schema_metadata")
        .and_then(serde_json::Value::as_object)
        .expect("output_schema_metadata must be present");
    let required_case_report_fields = output_schema_metadata
        .get("required_case_report_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_case_report_fields must be present");
    let required_deterministic_check_fields = output_schema_metadata
        .get("required_deterministic_check_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_deterministic_check_fields must be present");
    let required_statistical_check_fields = output_schema_metadata
        .get("required_statistical_check_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_statistical_check_fields must be present");

    let first_case_report = serialized_report["case_reports"][0]
        .as_object()
        .expect("first case report should be an object");
    for required_field in required_case_report_fields {
        let required_field = required_field
            .as_str()
            .expect("required case report field names should be strings");
        assert!(
            first_case_report.contains_key(required_field),
            "case report is missing required field `{required_field}`",
        );
    }

    let first_deterministic_check = serialized_report["case_reports"][0]["deterministic_checks"][0]
        .as_object()
        .expect("first deterministic check should be an object");
    for required_field in required_deterministic_check_fields {
        let required_field = required_field
            .as_str()
            .expect("required deterministic-check field names should be strings");
        assert!(
            first_deterministic_check.contains_key(required_field),
            "deterministic check is missing required field `{required_field}`",
        );
    }

    let first_statistical_check = serialized_report["case_reports"][0]["statistical_checks"][0]
        .as_object()
        .expect("first statistical check should be an object");
    for required_field in required_statistical_check_fields {
        let required_field = required_field
            .as_str()
            .expect("required statistical-check field names should be strings");
        assert!(
            first_statistical_check.contains_key(required_field),
            "statistical check is missing required field `{required_field}`",
        );
    }
}

fn build_validation_scenarios() -> Vec<SufficientStatisticsMultiIntervalValidationScenario> {
    vec![
        SufficientStatisticsMultiIntervalValidationScenario {
            scenario_id: "short_horizon_validation".to_string(),
            horizon_category: SufficientStatisticsValidationHorizonCategory::Short,
            run_request: build_run_request(71, 4),
        },
        SufficientStatisticsMultiIntervalValidationScenario {
            scenario_id: "medium_horizon_validation".to_string(),
            horizon_category: SufficientStatisticsValidationHorizonCategory::Medium,
            run_request: build_run_request(73, 12),
        },
        SufficientStatisticsMultiIntervalValidationScenario {
            scenario_id: "long_horizon_validation".to_string(),
            horizon_category: SufficientStatisticsValidationHorizonCategory::Long,
            run_request: build_run_request(79, 24),
        },
    ]
}

fn build_run_request(
    seed: u64,
    horizon_interval_count: usize,
) -> SufficientStatisticsRiskRunRequest {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.9,
        1.1,
        HawkesKernelLoadings {
            plus_plus: 0.21,
            plus_minus: 0.09,
            minus_plus: 0.12,
            minus_minus: 0.23,
        },
        HawkesKernelLoadings {
            plus_plus: 0.04,
            plus_minus: 0.01,
            minus_plus: 0.02,
            minus_minus: 0.03,
        },
    )
    .expect("fixture kernel parameters should construct");

    let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
        kernel_parameters,
        GammaSeverityMarkConfiguration::try_new(1.7, 0.8)
            .expect("fixture gamma mark configuration should construct"),
    )
    .expect("fixture equation specification should construct");

    let model_inputs = SufficientStatisticsRiskModelInputs::try_new(
        equation_specification,
        NigSharedPlusShiftConfiguration::try_new(2.4, -0.75, 0.8, 0.12, -0.18)
            .expect("fixture NIG configuration should construct"),
        HawkesExcitationState {
            y_plus_plus: 0.45,
            y_plus_minus: 0.16,
            y_minus_plus: 0.26,
            y_minus_minus: 0.37,
        },
    )
    .expect("fixture model inputs should construct");

    let (s_h, nu_h_known, nu_h_surprise) = build_interval_factors(horizon_interval_count);
    let interval_factors =
        SufficientStatisticsRiskIntervalFactors::try_new(s_h, nu_h_known, nu_h_surprise, Some(1.0))
            .expect("interval factors should construct");

    SufficientStatisticsRiskRunRequest::try_new(
        seed,
        horizon_interval_count,
        1.0,
        model_inputs,
        interval_factors,
    )
    .expect("fixture run request should construct")
}

fn build_interval_factors(horizon_interval_count: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut s_h: Vec<f64> = Vec::with_capacity(horizon_interval_count);
    let mut nu_h_known: Vec<f64> = Vec::with_capacity(horizon_interval_count);
    let mut nu_h_surprise: Vec<f64> = Vec::with_capacity(horizon_interval_count);

    for interval_index in 0..horizon_interval_count {
        s_h.push(0.85 + (interval_index % 5) as f64 * 0.05);
        let nu_h_total = 0.03 + (interval_index % 4) as f64 * 0.01;
        let known_nu_h = nu_h_total * 0.6;
        nu_h_known.push(known_nu_h);
        nu_h_surprise.push(nu_h_total - known_nu_h);
    }

    (s_h, nu_h_known, nu_h_surprise)
}
