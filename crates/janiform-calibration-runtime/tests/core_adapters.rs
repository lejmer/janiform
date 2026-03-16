mod support;
use support::*;

#[test]
fn calibration_domain_precheck_report_shape_is_compatible_with_optimizer_consumers() {
    let report = precheck_hawkes_kernel_parameter_domain(
        0.85,
        1.25,
        HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: 0.1,
            minus_plus: 0.2,
            minus_minus: 0.7,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.05,
            minus_plus: 0.12,
            minus_minus: 0.0,
        },
    );

    assert_eq!(
        report.schema_version,
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.status, HawkesKernelValidationStatus::Pass);
    assert_eq!(report.summary.failed_check_count, 0);
    assert_eq!(
        report.summary.passed_check_count,
        report.summary.total_check_count
    );
    assert!(report.violations.is_empty());
}

#[test]
fn calibration_structured_precheck_report_exposes_deterministic_failure_order() {
    let report = precheck_structured_hawkes_kernel_parameter_domain(
        0.0,
        -1.0,
        HawkesStructuredAlphaParameterization::new(0.0, -0.1, -0.1, -0.2),
        HawkesStructuredRhoParameterization::new(0.0, -0.2, -0.2, -0.3),
        Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
        -1.0,
    );

    assert_eq!(report.status, HawkesKernelValidationStatus::Fail);
    assert_eq!(
        report
            .violations
            .iter()
            .map(|violation| violation.field)
            .collect::<Vec<_>>(),
        vec![
            "lambda_bar",
            "beta",
            "structured_alpha.a_self",
            "structured_alpha.a_cross",
            "structured_alpha.delta_self",
            "structured_alpha.delta_cross",
            "alpha.plus_minus",
            "alpha.minus_plus",
            "alpha.minus_minus",
            "structured_rho.r_self",
            "structured_rho.r_cross",
            "structured_rho.delta_self_r",
            "structured_rho.delta_cross_r",
            "rho.plus_minus",
            "rho.minus_plus",
            "rho.minus_minus",
            "structured_rho.self_dominance_policy.minimum_self_to_cross_ratio",
            "stability.numeric_tolerance",
        ]
    );
}

#[test]
fn objective_composition_reports_weighted_sum_and_per_term_diagnostics() {
    let objective_terms = vec![
        CalibrationObjectiveTermInput {
            term_id: "tail_fit".to_string(),
            weight: 2.0,
            observed_score: 0.25,
        },
        CalibrationObjectiveTermInput {
            term_id: "activity_clustering".to_string(),
            weight: 1.5,
            observed_score: 0.4,
        },
    ];

    let report = compose_calibration_weighted_objective(objective_terms.as_slice())
        .expect("objective composition should succeed");

    assert_eq!(
        report.schema_version,
        CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION
    );
    assert_eq!(report.objective_terms.len(), 2);
    assert!((report.objective_terms[0].weighted_contribution - 0.5).abs() < 1e-12);
    assert!((report.objective_terms[1].weighted_contribution - 0.6).abs() < 1e-12);
    assert!((report.weighted_objective_value - 1.1).abs() < 1e-12);
}

#[test]
fn constrained_optimizer_adapter_configuration_and_failure_paths_are_validated() {
    let solver_settings = CalibrationOptimizerSettings::try_new("lbfgs", 25, 1e-8, 1e-6, 10.0)
        .expect("solver settings should validate");
    let duplicate_bounds = vec![
        CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
            .expect("first bound should validate"),
        CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
            .expect("second bound should validate"),
    ];

    let duplicate_bound_error = CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        7,
        solver_settings.clone(),
        duplicate_bounds,
    )
    .expect_err("duplicate parameter bounds must be rejected");
    assert!(matches!(
        duplicate_bound_error,
        CalibrationOptimizerAdapterConfigurationError::DuplicateParameterBound { .. }
    ));

    let valid_configuration = CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        7,
        solver_settings,
        vec![
            CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
                .expect("bound should validate"),
        ],
    )
    .expect("configuration should validate");

    let missing_parameter_error = run_constrained_optimizer_adapter(
        &valid_configuration,
        &[CalibrationOptimizerAdapterCandidateInput {
            candidate_id: "candidate_001".to_string(),
            objective_terms: vec![CalibrationObjectiveTermInput {
                term_id: "objective_1".to_string(),
                weight: 1.0,
                observed_score: 0.5,
            }],
            parameter_values: Vec::new(),
            external_constraint_violation_count: 0,
        }],
    )
    .expect_err("missing parameter value for configured bound must fail");

    assert!(matches!(
        missing_parameter_error,
        CalibrationOptimizerAdapterError::MissingBoundedParameterValue { .. }
    ));
}

#[test]
fn constrained_optimizer_adapter_is_deterministic_for_fixed_seed() {
    let configuration = CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        991,
        CalibrationOptimizerSettings::try_new("lbfgs", 50, 1e-8, 1e-6, 2.0)
            .expect("solver settings should validate"),
        vec![
            CalibrationOptimizerParameterBound::try_new("beta", 0.0, 10.0)
                .expect("bound should validate"),
        ],
    )
    .expect("configuration should validate");

    let tie_candidates = vec![
        CalibrationOptimizerAdapterCandidateInput {
            candidate_id: "candidate_a".to_string(),
            objective_terms: vec![CalibrationObjectiveTermInput {
                term_id: "objective".to_string(),
                weight: 1.0,
                observed_score: 1.0,
            }],
            parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                parameter_name: "beta".to_string(),
                value: 1.0,
            }],
            external_constraint_violation_count: 0,
        },
        CalibrationOptimizerAdapterCandidateInput {
            candidate_id: "candidate_b".to_string(),
            objective_terms: vec![CalibrationObjectiveTermInput {
                term_id: "objective".to_string(),
                weight: 1.0,
                observed_score: 1.0,
            }],
            parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                parameter_name: "beta".to_string(),
                value: 1.0,
            }],
            external_constraint_violation_count: 0,
        },
    ];

    let first_output = run_constrained_optimizer_adapter(&configuration, tie_candidates.as_slice())
        .expect("optimizer adapter run should succeed");
    let second_output =
        run_constrained_optimizer_adapter(&configuration, tie_candidates.as_slice())
            .expect("second optimizer adapter run should succeed");

    assert_eq!(
        first_output.metadata.schema_version,
        CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
    );
    assert_eq!(first_output, second_output);
    assert!(first_output.selected_candidate_id.is_some());
}

#[test]
fn calibration_esscher_transform_adapter_delegates_to_shared_module() {
    let parameters =
        NigParameters::try_new(2.3, -0.8, 0.7, 0.1).expect("fixture parameters should construct");

    let transformed = transform_nig_exponent_for_calibration_under_esscher_tilt(
        NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.4, -0.2), -0.05),
    )
    .expect("Esscher transform should evaluate");

    assert!(transformed.transformed_exponent.is_finite());
}

#[test]
fn calibration_martingale_adapter_delegates_to_shared_module() {
    let report = evaluate_calibration_martingale_condition_under_esscher_tilt(
        EsscherMartingaleConditionInput::new(ComplexScalar::new(0.12, 0.0), 0.1, 0.03),
    )
    .expect("martingale condition input should validate");

    assert!(report.is_satisfied);
    assert!(report.residual_norm <= report.tolerance);
}

#[test]
fn calibration_consumer_observation_builder_accepts_valid_pricing_hooks() {
    let hooks = VanillaOptionPricingCalibrationHooks::try_new(
        8.5,
        -0.03,
        1.08,
        0.5,
        None,
        Some(false),
        Some(true),
        Some(true),
    )
    .expect("pricing hooks should construct");

    let observation = build_option_surface_calibration_observation_from_pricing_hooks(hooks)
        .expect("calibration observation conversion should succeed");

    assert_eq!(observation.option_price, hooks.option_price);
    assert_eq!(observation.log_moneyness, hooks.log_moneyness);
    assert_eq!(observation.forward_moneyness, hooks.forward_moneyness);
    assert_eq!(observation.maturity_interval, hooks.maturity_interval);
    assert_eq!(
        observation.integration_numeric_quality_is_poor,
        hooks.integration_numeric_quality_is_poor
    );
    assert_eq!(
        observation.integration_convergence_check_passed,
        hooks.integration_convergence_check_passed
    );
    assert_eq!(
        observation.integration_truncation_check_passed,
        hooks.integration_truncation_check_passed
    );
}

#[test]
fn calibration_consumer_observation_builder_rejects_invalid_pricing_hooks() {
    let invalid_hooks = VanillaOptionPricingCalibrationHooks::new(
        8.5,
        -0.03,
        0.0,
        0.5,
        None,
        Some(false),
        Some(true),
        Some(true),
    );

    let error = build_option_surface_calibration_observation_from_pricing_hooks(invalid_hooks)
        .expect_err("invalid pricing hooks should fail calibration conversion");

    assert!(matches!(
        error,
        OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { .. }
    ));
}
