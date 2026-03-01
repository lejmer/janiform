#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{TimeZone, Utc};

    use crate::calendar::{
        ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, TimestampEventInputRecord,
    };
    use crate::calibration::{
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
        CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
        CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
        CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
        CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
        CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
        CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY,
        CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
        CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationCallerSurface,
        CalibrationExecutionMode, CalibrationMemoryCheckpointStore, CalibrationObjectiveTermInput,
        CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
        CalibrationOptimizerAdapterError, CalibrationOptimizerCandidateParameterValue,
        CalibrationOptimizerParameterBound, CalibrationOptimizerSettings,
        CalibrationResumeSettings, CalibrationResumeSettingsError, CalibrationRunContract,
        CalibrationRunContractError, CalibrationRunDataInputs, CalibrationRunSeedSettings,
        CalibrationRunSpec, CalibrationRunSpecError, CalibrationRunSpecErrorClassification,
        CalibrationStage, CalibrationStageDependency, CalibrationStageExecutionStatus,
        CalibrationStageGraph, CalibrationStageGraphError, CalibrationStageHandoff,
        CalibrationStageHandoffError, CalibrationStageSeedOverride, CalibrationStageSeedSource,
        CalibrationStageSolverSettings, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
        CalibrationStagedRunStageReport, HawkesStageCandidateInput, HawkesStageExecutionFailure,
        HawkesStageExecutionInput, HawkesStageObjectiveTermInput,
        OptionSurfaceCalibrationObservationError, ReturnComponentsDetectedCloseOpenGapCandidate,
        ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsKnownNewsCalendarInput,
        ReturnComponentsRealizedKnownNewsReturnSample,
        ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageExecutionFailure,
        ReturnComponentsStageExecutionInput, ReturnComponentsStageObjectiveMetricWeights,
        ReturnComponentsStageSignalSourceWindow, SeasonalityStageExecutionFailure,
        SeasonalityStageExecutionInput,
        build_option_surface_calibration_observation_from_pricing_hooks,
        calibration_checkpoint_reference_for_stage, calibration_last_success_checkpoint_reference,
        compose_calibration_weighted_objective, default_calibration_execution_mode_for_surface,
        derive_calibration_stage_seed_from_global_seed,
        evaluate_calibration_martingale_condition_under_esscher_tilt,
        precheck_hawkes_kernel_parameter_domain,
        precheck_structured_hawkes_kernel_parameter_domain,
        resolve_calibration_execution_mode_for_surface, resolve_calibration_stage_execution_order,
        resolve_calibration_stage_seed_metadata, run_constrained_optimizer_adapter,
        run_hawkes_calibration_stage, run_return_components_calibration_stage,
        run_seasonality_calibration_stage, run_staged_calibration_with_checkpointing,
        transform_nig_exponent_for_calibration_under_esscher_tilt,
    };
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
        HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };
    use crate::news::{
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
    };
    use crate::nig::{ComplexScalar, NigParameters, NigSharedPlusShiftConfiguration};
    use crate::pricing::VanillaOptionPricingCalibrationHooks;
    use crate::risk_neutral::{EsscherMartingaleConditionInput, NigEsscherTransformationInput};
    use serde_json::json;

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
            crate::calibration::CalibrationOptimizerAdapterConfigurationError::DuplicateParameterBound { .. }
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

        let first_output =
            run_constrained_optimizer_adapter(&configuration, tie_candidates.as_slice())
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
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");

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

    #[test]
    fn calibration_run_spec_round_trip_serialization_preserves_contract_fields() {
        let run_spec = build_valid_run_spec();

        assert_eq!(
            CalibrationRunSpec::schema_version(),
            CALIBRATION_RUN_SPEC_SCHEMA_VERSION
        );

        let serialized = serde_json::to_value(&run_spec).expect("run spec should serialize");
        assert_eq!(serialized["run_id"], json!("run-001"));
        assert_eq!(serialized["execution_mode"], json!("memory"));
        assert_eq!(serialized["selected_stages"][0], json!("seasonality"));
        assert_eq!(
            serialized["optimizer_settings_by_stage"][1]["stage"],
            json!("hawkes")
        );
        assert_eq!(serialized["seed_settings"]["global_seed"], json!(17));
        assert!(serialized.get("stage_handoffs").is_none());
        assert!(serialized.get("resume_settings").is_none());

        let parsed: CalibrationRunSpec =
            serde_json::from_value(serialized).expect("run spec should deserialize");
        assert_eq!(parsed, run_spec);
    }

    #[test]
    fn calibration_run_spec_deserialization_rejects_invalid_execution_mode() {
        let mut serialized = serde_json::to_value(build_valid_run_spec())
            .expect("run spec fixture should serialize");
        serialized["execution_mode"] = json!("unsupported_mode");

        let parse_result = serde_json::from_value::<CalibrationRunSpec>(serialized);
        assert!(parse_result.is_err());
    }

    #[test]
    fn calibration_stage_seed_derivation_is_deterministic_and_honors_overrides() {
        let run_spec_with_overrides = build_valid_run_spec();
        let seasonality_seed = resolve_calibration_stage_seed_metadata(
            &run_spec_with_overrides,
            CalibrationStage::Seasonality,
        );
        let hawkes_seed = resolve_calibration_stage_seed_metadata(
            &run_spec_with_overrides,
            CalibrationStage::Hawkes,
        );
        let return_seed_with_overrides = resolve_calibration_stage_seed_metadata(
            &run_spec_with_overrides,
            CalibrationStage::ReturnComponents,
        );

        assert_eq!(seasonality_seed.run_global_seed, 17);
        assert_eq!(seasonality_seed.stage_seed, 101);
        assert_eq!(
            seasonality_seed.stage_seed_source,
            CalibrationStageSeedSource::StageOverride
        );
        assert_eq!(hawkes_seed.stage_seed, 202);
        assert_eq!(
            hawkes_seed.stage_seed_source,
            CalibrationStageSeedSource::StageOverride
        );
        assert_eq!(
            return_seed_with_overrides.stage_seed_source,
            CalibrationStageSeedSource::DerivedFromGlobalSeed
        );
        assert_eq!(
            return_seed_with_overrides.stage_seed,
            derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
        );
        assert_eq!(
            CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            "global_seed_splitmix64_per_stage_with_override_precedence.v1"
        );

        let run_spec_without_overrides = CalibrationRunSpec::try_new(
            "run-derived-seeds-only",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            CalibrationRunSeedSettings::try_new(17, Vec::new())
                .expect("seed settings without overrides should validate"),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate");

        let derived_seasonality_seed = resolve_calibration_stage_seed_metadata(
            &run_spec_without_overrides,
            CalibrationStage::Seasonality,
        );
        let derived_hawkes_seed = resolve_calibration_stage_seed_metadata(
            &run_spec_without_overrides,
            CalibrationStage::Hawkes,
        );
        let derived_return_seed = resolve_calibration_stage_seed_metadata(
            &run_spec_without_overrides,
            CalibrationStage::ReturnComponents,
        );

        assert_eq!(
            derived_seasonality_seed,
            resolve_calibration_stage_seed_metadata(
                &run_spec_without_overrides,
                CalibrationStage::Seasonality
            )
        );
        assert_eq!(
            derived_seasonality_seed.stage_seed,
            derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::Seasonality)
        );
        assert_eq!(
            derived_hawkes_seed.stage_seed,
            derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::Hawkes)
        );
        assert_eq!(
            derived_return_seed.stage_seed,
            derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
        );
        assert_ne!(
            derived_seasonality_seed.stage_seed,
            derived_hawkes_seed.stage_seed
        );
        assert_ne!(
            derived_hawkes_seed.stage_seed,
            derived_return_seed.stage_seed
        );
    }

    #[test]
    fn calibration_execution_mode_defaults_are_surface_specific_and_documented() {
        assert_eq!(
            CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
            "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
        );
        assert_eq!(
            default_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli),
            CalibrationExecutionMode::Disk
        );
        assert_eq!(
            default_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::PythonImportableApi
            ),
            CalibrationExecutionMode::Memory
        );
    }

    #[test]
    fn calibration_execution_mode_resolution_honors_explicit_overrides_per_surface() {
        assert_eq!(
            resolve_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::Cli,
                Some(CalibrationExecutionMode::Memory)
            ),
            CalibrationExecutionMode::Memory
        );
        assert_eq!(
            resolve_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::PythonImportableApi,
                Some(CalibrationExecutionMode::Disk)
            ),
            CalibrationExecutionMode::Disk
        );
    }

    #[test]
    fn calibration_run_spec_for_caller_surface_applies_default_execution_mode() {
        let cli_spec = CalibrationRunSpec::try_new(
            "run-cli-default",
            build_data_inputs(),
            resolve_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli, None),
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .expect("cli default run spec should validate");
        assert_eq!(cli_spec.execution_mode, CalibrationExecutionMode::Disk);

        let python_spec = CalibrationRunSpec::try_new(
            "run-python-default",
            build_data_inputs(),
            resolve_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::PythonImportableApi,
                None,
            ),
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .expect("python default run spec should validate");
        assert_eq!(python_spec.execution_mode, CalibrationExecutionMode::Memory);
    }

    #[test]
    fn calibration_run_spec_mode_extensions_support_valid_memory_and_disk_configurations() {
        let memory_run_spec = CalibrationRunSpec::builder(
            "run-memory",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_stage_handoffs(vec![build_mode_aware_stage_handoff(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationExecutionMode::Memory,
            "artifact://seasonality/stage-001",
        )])
        .with_resume_settings(CalibrationResumeSettings::same_process(Some(
            CalibrationStage::Hawkes,
        )))
        .build()
        .expect("memory mode with in-memory handoff and same-process resume should validate");
        assert_eq!(
            memory_run_spec.execution_mode,
            CalibrationExecutionMode::Memory
        );

        let disk_run_spec = CalibrationRunSpec::builder(
            "run-disk",
            build_data_inputs(),
            CalibrationExecutionMode::Disk,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_stage_handoffs(vec![build_mode_aware_stage_handoff(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationExecutionMode::Disk,
            "artifact://seasonality/stage-001",
        )])
        .with_resume_settings(CalibrationResumeSettings::persisted_checkpoint_reference(
            "checkpoint://run-disk/last-success",
            None,
        ))
        .build()
        .expect("disk mode with persisted handoff and persisted resume should validate");
        assert_eq!(disk_run_spec.execution_mode, CalibrationExecutionMode::Disk);
    }

    #[test]
    fn calibration_run_spec_rejects_mode_incompatible_option_settings_with_deterministic_classification()
     {
        let memory_with_persisted_handoff = CalibrationRunSpec::builder(
            "run-invalid-memory-handoff",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_stage_handoffs(vec![
            CalibrationStageHandoff::persisted_artifact_reference(
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                "artifact://seasonality/stage-001",
            )
            .expect("persisted handoff fixture should be valid"),
        ])
        .build()
        .expect_err("memory mode must reject persisted artifact handoff");

        assert_eq!(
            memory_with_persisted_handoff.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            memory_with_persisted_handoff,
            CalibrationRunSpecError::InvalidStageHandoff {
                source: CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode { .. }
            }
        ));

        let memory_with_persisted_resume = CalibrationRunSpec::builder(
            "run-invalid-memory-resume",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_resume_settings(CalibrationResumeSettings::persisted_checkpoint_reference(
            "checkpoint://run-memory/last-success",
            None,
        ))
        .build()
        .expect_err("memory mode must reject persisted checkpoint resume");

        assert_eq!(
            memory_with_persisted_resume.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            memory_with_persisted_resume,
            CalibrationRunSpecError::InvalidResumeSettings {
                source: CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode
            }
        ));

        let disk_with_same_process_resume = CalibrationRunSpec::builder(
            "run-invalid-disk-resume",
            build_data_inputs(),
            CalibrationExecutionMode::Disk,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_resume_settings(CalibrationResumeSettings::same_process(None))
        .build()
        .expect_err("disk mode must reject same-process resume");

        assert_eq!(
            disk_with_same_process_resume.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            disk_with_same_process_resume,
            CalibrationRunSpecError::InvalidResumeSettings {
                source: CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
            }
        ));
    }

    #[test]
    fn calibration_stage_graph_validation_resolves_explicit_execution_order() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-ordered",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![
                CalibrationStage::ReturnComponents,
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate");

        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        assert_eq!(
            CalibrationStageGraph::schema_version(),
            CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
        );

        let serialized_graph =
            serde_json::to_value(&stage_graph).expect("stage graph should serialize");
        let parsed_graph: CalibrationStageGraph =
            serde_json::from_value(serialized_graph).expect("stage graph should deserialize");
        assert_eq!(parsed_graph, stage_graph);

        let execution_order = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect("valid graph should resolve an explicit topological execution order");

        assert_eq!(
            execution_order,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents
            ]
        );
    }

    #[test]
    fn calibration_stage_graph_rejects_unsupported_dependency_when_stage_is_unselected() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-unsupported-dependency",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .expect("run spec should validate");
        let stage_graph = CalibrationStageGraph::new(vec![CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Seasonality,
        }]);

        let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect_err("graph with unselected dependency stage must fail");
        assert!(matches!(
            error,
            CalibrationRunContractError::InvalidStageGraph {
                source: CalibrationStageGraphError::UnsupportedDependency {
                    stage: CalibrationStage::ReturnComponents,
                    depends_on: CalibrationStage::Seasonality,
                    ..
                }
            }
        ));
    }

    #[test]
    fn calibration_stage_graph_rejects_cycles() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-cycle",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate");

        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Seasonality,
                depends_on: CalibrationStage::ReturnComponents,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect_err("cyclic graph must fail");
        assert!(matches!(
            error,
            CalibrationRunContractError::InvalidStageGraph {
                source: CalibrationStageGraphError::CyclicDependency { .. }
            }
        ));
    }

    #[test]
    fn calibration_run_contract_captures_solver_settings_for_auditability() {
        let run_spec = build_valid_run_spec();
        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        let run_contract = CalibrationRunContract::try_new(run_spec.clone(), stage_graph)
            .expect("valid run contract should build");
        assert_eq!(
            run_contract.schema_version,
            CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION
        );
        assert_eq!(
            run_contract.solver_settings_capture_policy,
            CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY
        );
        assert_eq!(
            run_contract.stage_solver_settings,
            run_spec.optimizer_settings_by_stage
        );
        assert_eq!(
            run_contract.execution_stage_order,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents
            ]
        );

        let serialized =
            serde_json::to_value(&run_contract).expect("run contract should serialize");
        assert_eq!(
            serialized["stage_solver_settings"]
                .as_array()
                .map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            serialized["stage_solver_settings"][0]["optimizer_settings"]["solver_name"],
            json!("lbfgs")
        );
    }

    #[test]
    fn calibration_run_contract_construction_supports_memory_and_disk_modes() {
        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        for execution_mode in [
            CalibrationExecutionMode::Memory,
            CalibrationExecutionMode::Disk,
        ] {
            let run_spec = build_valid_run_spec_with_mode(execution_mode);
            let run_contract = CalibrationRunContract::try_new(run_spec, stage_graph.clone())
                .expect("run contract should build for both execution modes");
            assert_eq!(run_contract.run_spec.execution_mode, execution_mode);
        }
    }

    #[test]
    fn calibration_run_spec_rejects_missing_solver_settings_for_selected_stage() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-missing-solver",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            }],
        );

        assert!(matches!(
            run_spec,
            Err(CalibrationRunSpecError::MissingStageSolverSettings {
                stage: CalibrationStage::Hawkes
            })
        ));
    }

    #[test]
    fn staged_runner_emits_checkpoint_metadata_with_deterministic_identifiers_in_memory_mode() {
        let run_contract = build_run_contract_with_mode_and_resume(
            "run-checkpoint-memory",
            CalibrationExecutionMode::Memory,
            None,
        );
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let report = run_staged_calibration_with_checkpointing(
            &run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
        assert!(report.failure.is_none());
        assert_eq!(report.emitted_checkpoints.len(), 3);
        assert_eq!(
            checkpoint_store.checkpoint_count_for_run(run_contract.run_spec.run_id.as_str()),
            3
        );

        let first_checkpoint = report
            .emitted_checkpoints
            .first()
            .expect("first stage checkpoint should exist");

        assert_eq!(
            first_checkpoint.schema_version,
            CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION
        );
        assert_eq!(
            first_checkpoint.checkpoint_reference_policy,
            CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY
        );
        assert_eq!(
            first_checkpoint.disk_write_policy,
            CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY
        );
        assert_eq!(
            first_checkpoint.artifact_manifest_schema_version,
            CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
        );
        assert_eq!(
            first_checkpoint.checkpoint_reference,
            calibration_checkpoint_reference_for_stage(
                run_contract.run_spec.run_id.as_str(),
                CalibrationStage::Seasonality
            )
        );
        assert_eq!(
            first_checkpoint.completed_stage,
            CalibrationStage::Seasonality
        );
        assert_eq!(first_checkpoint.completed_stage_position, 0);
        assert_eq!(first_checkpoint.run_global_seed, 17);
        assert_eq!(first_checkpoint.completed_stage_seed, 101);
        assert_eq!(
            first_checkpoint.completed_stage_seed_source,
            CalibrationStageSeedSource::StageOverride
        );
        assert_eq!(
            first_checkpoint.stage_seed_derivation_policy,
            CALIBRATION_STAGE_SEED_DERIVATION_POLICY
        );

        let return_components_checkpoint = report
            .emitted_checkpoints
            .iter()
            .find(|checkpoint| checkpoint.completed_stage == CalibrationStage::ReturnComponents)
            .expect("return-components stage checkpoint should exist");

        assert_eq!(
            return_components_checkpoint.completed_stage_seed_source,
            CalibrationStageSeedSource::DerivedFromGlobalSeed
        );
        assert_eq!(
            return_components_checkpoint.completed_stage_seed,
            derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
        );
        assert_eq!(
            checkpoint_store
                .last_checkpoint_for_run(run_contract.run_spec.run_id.as_str())
                .expect("store should expose last checkpoint")
                .completed_stage,
            CalibrationStage::ReturnComponents
        );
    }

    #[test]
    fn staged_runner_reports_run_and_stage_seed_metadata_for_all_stages() {
        let run_contract = build_run_contract_with_mode_and_resume(
            "run-stage-seed-metadata",
            CalibrationExecutionMode::Memory,
            None,
        );
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let report = run_staged_calibration_with_checkpointing(
            &run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
        assert_eq!(report.stage_reports.len(), 3);

        for stage_report in &report.stage_reports {
            match stage_report {
                CalibrationStagedRunStageReport::Seasonality(seasonality_report) => {
                    let metadata = seasonality_report
                        .metadata
                        .as_ref()
                        .expect("seasonality stage metadata should be present");
                    assert_eq!(metadata.run_global_seed, 17);
                    assert_eq!(metadata.stage_seed, 101);
                    assert_eq!(
                        metadata.stage_seed_source,
                        CalibrationStageSeedSource::StageOverride
                    );
                    assert_eq!(
                        metadata.stage_seed_derivation_policy,
                        CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                    );
                }
                CalibrationStagedRunStageReport::Hawkes(hawkes_report) => {
                    let metadata = hawkes_report
                        .metadata
                        .as_ref()
                        .expect("hawkes stage metadata should be present");
                    assert_eq!(metadata.run_global_seed, 17);
                    assert_eq!(metadata.stage_seed, 202);
                    assert_eq!(
                        metadata.stage_seed_source,
                        CalibrationStageSeedSource::StageOverride
                    );
                    assert_eq!(
                        metadata.stage_seed_derivation_policy,
                        CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                    );
                }
                CalibrationStagedRunStageReport::ReturnComponents(return_components_report) => {
                    let metadata = return_components_report
                        .metadata
                        .as_ref()
                        .expect("return-components stage metadata should be present");
                    assert_eq!(metadata.run_global_seed, 17);
                    assert_eq!(
                        metadata.stage_seed,
                        derive_calibration_stage_seed_from_global_seed(
                            17,
                            CalibrationStage::ReturnComponents
                        )
                    );
                    assert_eq!(
                        metadata.stage_seed_source,
                        CalibrationStageSeedSource::DerivedFromGlobalSeed
                    );
                    assert_eq!(
                        metadata.stage_seed_derivation_policy,
                        CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                    );
                }
            }
        }
    }

    #[test]
    fn staged_runner_full_workflow_is_reproducible_across_repeated_runs_in_both_modes() {
        let calendar_registry = calibration_stage_test_registry();

        let memory_contract = build_run_contract_with_mode_and_resume(
            "run-repeat-memory",
            CalibrationExecutionMode::Memory,
            None,
        );
        let mut first_memory_checkpoint_store = CalibrationMemoryCheckpointStore::new();
        let first_memory_report = run_staged_calibration_with_checkpointing(
            &memory_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            Some(&mut first_memory_checkpoint_store),
            None,
        );
        let mut second_memory_checkpoint_store = CalibrationMemoryCheckpointStore::new();
        let second_memory_report = run_staged_calibration_with_checkpointing(
            &memory_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            Some(&mut second_memory_checkpoint_store),
            None,
        );

        assert_eq!(first_memory_report, second_memory_report);

        let checkpoint_directory = temporary_checkpoint_directory("repeat-full-disk");
        let disk_contract = build_run_contract_with_mode_and_resume(
            "run-repeat-disk",
            CalibrationExecutionMode::Disk,
            None,
        );
        let first_disk_report = run_staged_calibration_with_checkpointing(
            &disk_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            None,
            Some(checkpoint_directory.as_path()),
        );
        let second_disk_report = run_staged_calibration_with_checkpointing(
            &disk_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            None,
            Some(checkpoint_directory.as_path()),
        );

        assert_eq!(first_disk_report, second_disk_report);

        let _ = fs::remove_dir_all(checkpoint_directory);
    }

    #[test]
    fn staged_runner_resume_from_last_successful_stage_works_in_memory_mode() {
        let run_id = "run-memory-resume-last";
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
        let initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            Some(&mut checkpoint_store),
            None,
        );
        assert_eq!(
            initial_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(initial_report.emitted_checkpoints.len(), 2);
        assert_eq!(
            initial_report.failure,
            Some(CalibrationStagedRunFailure::MissingStageInput {
                stage: CalibrationStage::ReturnComponents
            })
        );

        let resume_run_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings::same_process(None)),
        );
        let resume_report = run_staged_calibration_with_checkpointing(
            &resume_run_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(
            resume_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_report.resumed_from_last_successful_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(
            resume_report.started_from_stage,
            Some(CalibrationStage::ReturnComponents)
        );
        assert_eq!(resume_report.stage_reports.len(), 1);
        assert_eq!(resume_report.emitted_checkpoints.len(), 1);
    }

    #[test]
    fn staged_runner_resume_from_selected_stage_works_in_memory_mode() {
        let run_id = "run-memory-resume-stage";
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
        let _initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            Some(&mut checkpoint_store),
            None,
        );

        let resume_run_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings::same_process(Some(
                CalibrationStage::Hawkes,
            ))),
        );
        let resume_report = run_staged_calibration_with_checkpointing(
            &resume_run_contract,
            &calendar_registry,
            staged_run_inputs(false, true, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(
            resume_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_report.started_from_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(resume_report.stage_reports.len(), 2);
        assert_eq!(resume_report.emitted_checkpoints.len(), 2);
    }

    #[test]
    fn staged_runner_resume_from_last_and_selected_stage_work_in_disk_mode() {
        let run_id = "run-disk-resume";
        let checkpoint_directory = temporary_checkpoint_directory("disk-resume");
        let calendar_registry = calibration_stage_test_registry();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Disk, None);
        let initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            None,
            Some(checkpoint_directory.as_path()),
        );
        assert_eq!(
            initial_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(initial_report.emitted_checkpoints.len(), 2);
        assert!(
            checkpoint_directory
                .join(run_id)
                .join("hawkes.checkpoint.json")
                .exists()
        );
        assert!(
            checkpoint_directory
                .join(run_id)
                .join("last-success.checkpoint.json")
                .exists()
        );

        let resume_from_last_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings::persisted_checkpoint_reference(
                calibration_last_success_checkpoint_reference(run_id),
                None,
            )),
        );
        let resume_from_last_report = run_staged_calibration_with_checkpointing(
            &resume_from_last_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            Some(checkpoint_directory.as_path()),
        );

        assert_eq!(
            resume_from_last_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_from_last_report.started_from_stage,
            Some(CalibrationStage::ReturnComponents)
        );
        assert_eq!(resume_from_last_report.stage_reports.len(), 1);

        let resume_from_stage_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings::persisted_checkpoint_reference(
                calibration_last_success_checkpoint_reference(run_id),
                Some(CalibrationStage::Hawkes),
            )),
        );
        let resume_from_stage_report = run_staged_calibration_with_checkpointing(
            &resume_from_stage_contract,
            &calendar_registry,
            staged_run_inputs(false, true, true),
            None,
            Some(checkpoint_directory.as_path()),
        );
        assert_eq!(
            resume_from_stage_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_from_stage_report.started_from_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(resume_from_stage_report.stage_reports.len(), 2);

        let _ = fs::remove_dir_all(checkpoint_directory);
    }

    #[test]
    fn staged_runner_rejects_incompatible_checkpoints_and_mode_specific_runtime_requirements() {
        let checkpoint_directory = temporary_checkpoint_directory("incompatible-checkpoint");
        let calendar_registry = calibration_stage_test_registry();

        let source_run_contract = build_run_contract_with_mode_and_resume(
            "run-source",
            CalibrationExecutionMode::Disk,
            None,
        );
        let _source_report = run_staged_calibration_with_checkpointing(
            &source_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            None,
            Some(checkpoint_directory.as_path()),
        );

        let incompatible_run_contract = build_run_contract_with_mode_and_resume(
            "run-target",
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings::persisted_checkpoint_reference(
                calibration_last_success_checkpoint_reference("run-source"),
                None,
            )),
        );
        let incompatible_report = run_staged_calibration_with_checkpointing(
            &incompatible_run_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            Some(checkpoint_directory.as_path()),
        );

        assert_eq!(
            incompatible_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            incompatible_report.failure,
            Some(CalibrationStagedRunFailure::CheckpointCompatibilityFailed { .. })
        ));

        let memory_resume_contract = build_run_contract_with_mode_and_resume(
            "run-memory-missing-store",
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings::same_process(None)),
        );
        let missing_store_report = run_staged_calibration_with_checkpointing(
            &memory_resume_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            None,
        );

        assert_eq!(
            missing_store_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(
            missing_store_report.failure,
            Some(CalibrationStagedRunFailure::MissingMemoryCheckpointStore)
        );

        let disk_missing_directory_contract = build_run_contract_with_mode_and_resume(
            "run-disk-missing-directory",
            CalibrationExecutionMode::Disk,
            None,
        );
        let missing_directory_report = run_staged_calibration_with_checkpointing(
            &disk_missing_directory_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            None,
            None,
        );

        assert_eq!(
            missing_directory_report.failure,
            Some(CalibrationStagedRunFailure::MissingDiskCheckpointDirectory)
        );

        let _ = fs::remove_dir_all(checkpoint_directory);
    }

    #[test]
    fn seasonality_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
        let calendar_registry = calibration_stage_test_registry();
        let input_events = calibration_stage_timestamp_fixture();

        let memory_report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(input_events.clone()),
        );
        let disk_report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Disk),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(input_events),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .normalized_stage_payload
            .as_ref()
            .expect("memory report should include normalized payload");
        let disk_payload = disk_report
            .normalized_stage_payload
            .as_ref()
            .expect("disk report should include normalized payload");
        assert_eq!(memory_payload, disk_payload);

        let memory_handoff = memory_report
            .handoffs
            .first()
            .expect("memory report should include one handoff");
        assert!(memory_handoff.in_memory_payload_available);
        assert!(memory_handoff.artifact_reference.is_none());

        let disk_handoff = disk_report
            .handoffs
            .first()
            .expect("disk report should include one handoff");
        assert!(!disk_handoff.in_memory_payload_available);
        assert_eq!(
            disk_handoff.artifact_reference.as_deref(),
            Some("artifact://seasonality/run-001")
        );
    }

    #[test]
    fn seasonality_stage_execution_reports_missing_input_and_invalid_config_failures() {
        let calendar_registry = calibration_stage_test_registry();
        let run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory);

        let missing_input_report = run_seasonality_calibration_stage(
            &run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(Vec::new()),
        );
        assert_eq!(
            missing_input_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(
            missing_input_report.failure,
            Some(SeasonalityStageExecutionFailure::MissingTimestampEventRecords)
        );

        let mut invalid_config_input = SeasonalityStageExecutionInput::from_timestamp_event_records(
            calibration_stage_timestamp_fixture(),
        );
        invalid_config_input.settings.sparse_hour_event_threshold = 0;

        let invalid_config_report =
            run_seasonality_calibration_stage(&run_spec, &calendar_registry, invalid_config_input);
        assert_eq!(
            invalid_config_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            invalid_config_report.failure,
            Some(SeasonalityStageExecutionFailure::TimestampPreprocessingFailed { .. })
        ));
    }

    #[test]
    fn seasonality_stage_execution_metadata_is_deterministic_under_fixed_seed_and_config() {
        let calendar_registry = calibration_stage_test_registry();
        let memory_run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_seasonality_calibration_stage(
            &memory_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let second_memory_report = run_seasonality_calibration_stage(
            &memory_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let first_disk_report = run_seasonality_calibration_stage(
            &disk_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let second_disk_report = run_seasonality_calibration_stage(
            &disk_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.diagnostics_snapshot_fingerprint_hex,
            first_disk_metadata.diagnostics_snapshot_fingerprint_hex
        );
        assert_eq!(
            first_memory_metadata.estimator_version,
            CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION
        );
    }

    #[test]
    fn seasonality_stage_execution_supports_standalone_partial_pipeline_runs() {
        let calendar_registry = calibration_stage_test_registry();

        let standalone_seed_settings = CalibrationRunSeedSettings::try_new(
            17,
            vec![CalibrationStageSeedOverride {
                stage: CalibrationStage::Seasonality,
                seed: 101,
            }],
        )
        .expect("standalone seed settings should validate");

        let standalone_run_spec = CalibrationRunSpec::try_new(
            "run-seasonality-only",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality],
            standalone_seed_settings,
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            }],
        )
        .expect("standalone seasonality run spec should validate");

        let report = run_seasonality_calibration_stage(
            &standalone_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
        assert!(report.handoffs.is_empty());
        assert!(report.failure.is_none());
    }

    #[test]
    fn hawkes_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
        let seasonality_payload = build_seasonality_stage_success_payload();

        let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload.clone(),
                hawkes_stage_candidate_fixture(),
            ),
        );
        let disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted stage payload");
        let disk_payload = disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted stage payload");

        assert_eq!(
            memory_payload.selected_candidate_id,
            "candidate_stable_low_loss"
        );
        assert_eq!(
            memory_payload.selected_candidate_id,
            disk_payload.selected_candidate_id
        );
        assert_eq!(
            memory_payload.fitted_parameters.kernel_parameters,
            disk_payload.fitted_parameters.kernel_parameters
        );
        assert_eq!(
            memory_payload.candidate_diagnostics,
            disk_payload.candidate_diagnostics
        );

        assert_eq!(
            memory_payload.seasonality_dataset_id.as_deref(),
            Some(seasonality_payload.dataset_id.as_str())
        );
        assert!(disk_payload.seasonality_dataset_id.is_none());
        assert_eq!(
            disk_payload.seasonality_artifact_reference.as_deref(),
            Some("artifact://seasonality/run-001")
        );

        let memory_handoff = memory_report
            .handoffs
            .first()
            .expect("memory report should include one handoff");
        assert!(memory_handoff.in_memory_payload_available);
        assert!(memory_handoff.artifact_reference.is_none());

        let disk_handoff = disk_report
            .handoffs
            .first()
            .expect("disk report should include one handoff");
        assert!(!disk_handoff.in_memory_payload_available);
        assert_eq!(
            disk_handoff.artifact_reference.as_deref(),
            Some("artifact://hawkes/run-001")
        );
    }

    #[test]
    fn hawkes_stage_execution_reports_constraint_violations_for_invalid_candidates() {
        let run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);

        let report = run_hawkes_calibration_stage(
            &run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                build_seasonality_stage_success_payload(),
                vec![hawkes_stage_constraint_violating_candidate_fixture()],
            ),
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);

        let failure = report
            .failure
            .as_ref()
            .expect("failure report should include details");
        match failure {
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            } => {
                assert_eq!(candidate_diagnostics.len(), 1);
                let candidate = &candidate_diagnostics[0];
                assert_eq!(candidate.candidate_id, "candidate_invalid_constraints");
                assert!(candidate.constraint_violation_count > 0);
                assert_eq!(
                    candidate.validation_report.status,
                    HawkesKernelValidationStatus::Fail
                );
            }
            _ => panic!("expected constraint failure diagnostics"),
        }
    }

    #[test]
    fn hawkes_stage_execution_is_deterministic_under_fixed_seed_and_solver_settings() {
        let seasonality_payload = build_seasonality_stage_success_payload();
        let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload.clone(),
                hawkes_stage_candidate_fixture(),
            ),
        );
        let second_memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload,
                hawkes_stage_candidate_fixture(),
            ),
        );
        let first_disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );
        let second_disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );

        let first_memory_payload = first_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted payload");
        let second_memory_payload = second_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("second memory report should include fitted payload");
        let first_disk_payload = first_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted payload");
        let second_disk_payload = second_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("second disk report should include fitted payload");

        assert_eq!(first_memory_payload, second_memory_payload);
        assert_eq!(first_disk_payload, second_disk_payload);
        assert_eq!(
            first_memory_payload.selected_candidate_id,
            first_disk_payload.selected_candidate_id
        );
        assert_eq!(
            first_memory_payload.fitted_parameters.kernel_parameters,
            first_disk_payload.fitted_parameters.kernel_parameters
        );
        assert_eq!(
            first_memory_payload.candidate_diagnostics,
            first_disk_payload.candidate_diagnostics
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.solver_settings,
            first_disk_metadata.solver_settings
        );
    }

    #[test]
    fn return_components_stage_execution_succeeds_for_memory_and_disk_pipeline_handoffs() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload.clone(),
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        let disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_generation_fixture(),
            ),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted stage payload");
        let disk_payload = disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted stage payload");

        assert!(!memory_payload.selected_candidate_id.trim().is_empty());
        assert_eq!(
            memory_payload.selected_candidate_id,
            disk_payload.selected_candidate_id
        );
        assert_eq!(
            memory_payload.fitted_parameters,
            disk_payload.fitted_parameters
        );
        assert_eq!(
            memory_payload.candidate_diagnostics,
            disk_payload.candidate_diagnostics
        );
        assert_eq!(
            memory_payload
                .activity_selected_hawkes_candidate_id
                .as_deref(),
            Some(hawkes_payload.selected_candidate_id.as_str())
        );
        assert!(disk_payload.activity_selected_hawkes_candidate_id.is_none());
        assert_eq!(
            disk_payload.activity_artifact_reference.as_deref(),
            Some("artifact://hawkes/run-001")
        );
        assert_eq!(
            memory_payload.objective_metric_bucket_policy,
            "open_jump_closed_gap_activity_coupling_bucketed_weighted_terms.v1"
        );
        assert_eq!(
            memory_payload.candidate_generation_policy,
            "known_calendar_and_observed_jump_gap_signals_to_candidate_families.v1"
        );

        for candidate_diagnostic in &memory_payload.candidate_diagnostics {
            let term_ids = candidate_diagnostic
                .objective_terms
                .iter()
                .map(|objective_term| objective_term.term_id.as_str())
                .collect::<Vec<_>>();
            assert!(term_ids.contains(&"open_jump_fit"));
            assert!(term_ids.contains(&"closed_gap_fit"));
            assert!(term_ids.contains(&"activity_coupling_fit"));
            assert!(
                !candidate_diagnostic
                    .candidate_provenance
                    .source_windows
                    .is_empty()
            );
            assert!(
                !candidate_diagnostic
                    .candidate_provenance
                    .preprocessing_filters
                    .is_empty()
            );
            assert!(
                candidate_diagnostic
                    .objective_metric_buckets
                    .open_jump_fit
                    .is_finite()
            );
            assert!(
                candidate_diagnostic
                    .objective_metric_buckets
                    .closed_gap_fit
                    .is_finite()
            );
            assert!(
                candidate_diagnostic
                    .objective_metric_buckets
                    .activity_coupling_fit
                    .is_finite()
            );
        }
    }

    #[test]
    fn return_components_stage_execution_supports_standalone_runs_in_memory_and_disk_modes() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        let disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/standalone",
                return_components_stage_candidate_generation_fixture(),
            ),
        );

        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);
        assert!(memory_report.handoffs.is_empty());
        assert!(disk_report.handoffs.is_empty());
        assert!(memory_report.failure.is_none());
        assert!(disk_report.failure.is_none());
    }

    #[test]
    fn return_components_stage_execution_rejects_incomplete_candidate_generation_inputs() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);

        let mut candidate_generation_input = return_components_stage_candidate_generation_fixture();
        candidate_generation_input
            .detected_intraday_jump_candidates
            .clear();

        let report = run_return_components_calibration_stage(
            &run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                candidate_generation_input,
            ),
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
        assert!(matches!(
            report.failure,
            Some(ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput { .. })
        ));
    }

    #[test]
    fn return_components_stage_execution_reports_missing_dependency_stage_outputs() {
        let hawkes_payload = build_hawkes_stage_success_payload();

        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let memory_failure_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        assert_eq!(
            memory_failure_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            memory_failure_report.failure,
            Some(ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch { .. })
        ));

        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);
        let disk_failure_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        assert_eq!(
            disk_failure_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            disk_failure_report.failure,
            Some(ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch { .. })
        ));
    }

    #[test]
    fn return_components_stage_execution_is_deterministic_under_fixed_settings() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload.clone(),
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        let second_memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        let first_disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_generation_fixture(),
            ),
        );
        let second_disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_generation_fixture(),
            ),
        );

        let first_memory_payload = first_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted payload");
        let second_memory_payload = second_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("second memory report should include fitted payload");
        let first_disk_payload = first_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted payload");
        let second_disk_payload = second_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("second disk report should include fitted payload");

        assert_eq!(first_memory_payload, second_memory_payload);
        assert_eq!(first_disk_payload, second_disk_payload);
        assert_eq!(
            first_memory_payload.selected_candidate_id,
            first_disk_payload.selected_candidate_id
        );
        assert_eq!(
            first_memory_payload.fitted_parameters,
            first_disk_payload.fitted_parameters
        );
        assert_eq!(
            first_memory_payload.candidate_diagnostics,
            first_disk_payload.candidate_diagnostics
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.solver_settings,
            first_disk_metadata.solver_settings
        );
    }

    fn build_data_inputs() -> CalibrationRunDataInputs {
        CalibrationRunDataInputs::try_new(
            "XNYS",
            "dataset://synthetic/trades/v1",
            Some("dataset://synthetic/options/v1".to_string()),
        )
        .expect("data inputs should validate")
    }

    fn build_seed_settings() -> CalibrationRunSeedSettings {
        CalibrationRunSeedSettings::try_new(
            17,
            vec![
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Seasonality,
                    seed: 101,
                },
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Hawkes,
                    seed: 202,
                },
            ],
        )
        .expect("seed settings should validate")
    }

    fn build_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("lbfgs", 250, 1e-8, 1e-6, 10.0)
            .expect("solver settings should validate")
    }

    fn build_solver_settings_for_seasonality_and_hawkes() -> Vec<CalibrationStageSolverSettings> {
        vec![
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            },
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Hawkes,
                optimizer_settings: build_solver_settings(),
            },
        ]
    }

    fn build_solver_settings_for_all_stages() -> Vec<CalibrationStageSolverSettings> {
        vec![
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            },
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Hawkes,
                optimizer_settings: build_solver_settings(),
            },
            CalibrationStageSolverSettings {
                stage: CalibrationStage::ReturnComponents,
                optimizer_settings: build_solver_settings(),
            },
        ]
    }

    fn build_valid_run_spec() -> CalibrationRunSpec {
        build_valid_run_spec_with_mode(CalibrationExecutionMode::Memory)
    }

    fn build_valid_run_spec_with_mode(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        CalibrationRunSpec::try_new(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate")
    }

    fn build_run_contract_with_mode_and_resume(
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        resume_settings: Option<CalibrationResumeSettings>,
    ) -> CalibrationRunContract {
        let run_spec = {
            let builder = CalibrationRunSpec::builder(
                run_id,
                build_data_inputs(),
                execution_mode,
                vec![
                    CalibrationStage::Seasonality,
                    CalibrationStage::Hawkes,
                    CalibrationStage::ReturnComponents,
                ],
                build_seed_settings(),
                build_solver_settings_for_all_stages(),
            );

            let builder = if let Some(resume_settings) = resume_settings {
                builder.with_resume_settings(resume_settings)
            } else {
                builder
            };

            builder.build().expect("run spec should validate")
        };

        CalibrationRunContract::try_new(run_spec, default_stage_graph())
            .expect("run contract should validate")
    }

    fn default_stage_graph() -> CalibrationStageGraph {
        CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ])
    }

    fn build_mode_aware_stage_handoff(
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
        execution_mode: CalibrationExecutionMode,
        persisted_artifact_reference: &str,
    ) -> CalibrationStageHandoff {
        match execution_mode {
            CalibrationExecutionMode::Memory => {
                CalibrationStageHandoff::in_memory_output(from_stage, to_stage)
            }
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoff::persisted_artifact_reference(
                    from_stage,
                    to_stage,
                    persisted_artifact_reference,
                )
                .expect("disk-mode persisted handoff fixture should be valid")
            }
        }
    }

    fn staged_run_inputs(
        include_seasonality_input: bool,
        include_hawkes_candidates: bool,
        include_return_candidate_generation_input: bool,
    ) -> CalibrationStagedRunInputs {
        CalibrationStagedRunInputs {
            seasonality_stage_input: if include_seasonality_input {
                Some(
                    SeasonalityStageExecutionInput::from_timestamp_event_records(
                        calibration_stage_timestamp_fixture(),
                    ),
                )
            } else {
                None
            },
            hawkes_candidate_inputs: if include_hawkes_candidates {
                Some(hawkes_stage_candidate_fixture())
            } else {
                None
            },
            return_components_candidate_generation_input:
                if include_return_candidate_generation_input {
                    Some(return_components_stage_candidate_generation_fixture())
                } else {
                    None
                },
        }
    }

    fn temporary_checkpoint_directory(test_name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX epoch")
            .as_nanos();
        let checkpoint_directory =
            std::env::temp_dir().join(format!("janiform-checkpoints-{test_name}-{unique_suffix}"));

        if checkpoint_directory.exists() {
            let _ = fs::remove_dir_all(checkpoint_directory.as_path());
        }

        fs::create_dir_all(checkpoint_directory.as_path())
            .expect("temporary checkpoint directory should be created");

        checkpoint_directory
    }

    fn build_seasonality_stage_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        let stage_handoff = build_mode_aware_stage_handoff(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            execution_mode,
            "artifact://seasonality/run-001",
        );

        CalibrationRunSpec::builder(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            build_solver_settings_for_seasonality_and_hawkes(),
        )
        .with_stage_handoffs(vec![stage_handoff])
        .build()
        .expect("seasonality stage run spec should validate")
    }

    fn build_hawkes_stage_run_spec(execution_mode: CalibrationExecutionMode) -> CalibrationRunSpec {
        let seasonality_handoff = build_mode_aware_stage_handoff(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            execution_mode,
            "artifact://seasonality/run-001",
        );

        let hawkes_handoff = build_mode_aware_stage_handoff(
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            execution_mode,
            "artifact://hawkes/run-001",
        );

        CalibrationRunSpec::builder(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .with_stage_handoffs(vec![seasonality_handoff, hawkes_handoff])
        .build()
        .expect("hawkes stage run spec should validate")
    }

    fn build_return_components_stage_pipeline_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        build_hawkes_stage_run_spec(execution_mode)
    }

    fn build_return_components_stage_standalone_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        let seed_settings = CalibrationRunSeedSettings::try_new(
            23,
            vec![CalibrationStageSeedOverride {
                stage: CalibrationStage::ReturnComponents,
                seed: 303,
            }],
        )
        .expect("standalone return-components seed settings should validate");

        CalibrationRunSpec::try_new(
            "run-return-components-standalone",
            build_data_inputs(),
            execution_mode,
            vec![CalibrationStage::ReturnComponents],
            seed_settings,
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::ReturnComponents,
                optimizer_settings: build_solver_settings(),
            }],
        )
        .expect("standalone return-components run spec should validate")
    }

    fn build_seasonality_stage_success_payload()
    -> crate::calibration::SeasonalityStageNormalizedPayload {
        let calendar_registry = calibration_stage_test_registry();
        let report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        report
            .normalized_stage_payload
            .expect("seasonality stage fixture should produce normalized payload")
    }

    fn build_hawkes_stage_success_payload() -> crate::calibration::HawkesStageFittedPayload {
        let seasonality_payload = build_seasonality_stage_success_payload();
        let report = run_hawkes_calibration_stage(
            &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload,
                hawkes_stage_candidate_fixture(),
            ),
        );

        report
            .fitted_stage_payload
            .expect("hawkes stage fixture should produce fitted payload")
    }

    fn hawkes_stage_candidate_fixture() -> Vec<HawkesStageCandidateInput> {
        vec![
            HawkesStageCandidateInput {
                candidate_id: "candidate_high_loss".to_string(),
                lambda_bar: 0.85,
                beta: 1.50,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.40, 0.10, 0.08, 0.10,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.35, 0.10, 0.05, 0.10),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                objective_terms: vec![
                    HawkesStageObjectiveTermInput {
                        term_id: "event_rate_residual".to_string(),
                        weight: 1.0,
                        observed_score: 2.8,
                    },
                    HawkesStageObjectiveTermInput {
                        term_id: "clustering_autocorrelation".to_string(),
                        weight: 1.0,
                        observed_score: 1.4,
                    },
                ],
            },
            HawkesStageCandidateInput {
                candidate_id: "candidate_stable_low_loss".to_string(),
                lambda_bar: 0.80,
                beta: 1.60,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.35, 0.06, 0.07, 0.04,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.30, 0.08, 0.04, 0.05),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                objective_terms: vec![
                    HawkesStageObjectiveTermInput {
                        term_id: "event_rate_residual".to_string(),
                        weight: 1.0,
                        observed_score: 1.6,
                    },
                    HawkesStageObjectiveTermInput {
                        term_id: "clustering_autocorrelation".to_string(),
                        weight: 1.0,
                        observed_score: 0.7,
                    },
                ],
            },
        ]
    }

    fn hawkes_stage_constraint_violating_candidate_fixture() -> HawkesStageCandidateInput {
        HawkesStageCandidateInput {
            candidate_id: "candidate_invalid_constraints".to_string(),
            lambda_bar: -0.2,
            beta: 0.0,
            structured_alpha: HawkesStructuredAlphaParameterization::new(-0.1, -0.1, -0.1, -0.1),
            structured_rho: HawkesStructuredRhoParameterization::new(-0.2, -0.1, -0.1, -0.1),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
            stability_numeric_tolerance: -0.5,
            objective_terms: vec![HawkesStageObjectiveTermInput {
                term_id: "event_rate_residual".to_string(),
                weight: 1.0,
                observed_score: 1.0,
            }],
        }
    }

    fn return_components_stage_candidate_generation_fixture()
    -> ReturnComponentsStageCandidateGenerationInput {
        let source_window = ReturnComponentsStageSignalSourceWindow {
            window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
            window_end_utc_nanos: utc_nanos(2026, 2, 5, 0, 0, 0),
        };

        ReturnComponentsStageCandidateGenerationInput {
            known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
                source_window,
                scheduled_event_timestamps_utc_nanos: vec![
                    utc_nanos(2026, 2, 2, 14, 30, 0),
                    utc_nanos(2026, 2, 3, 15, 0, 0),
                    utc_nanos(2026, 2, 4, 16, 0, 0),
                ],
            },
            realized_known_news_return_samples: vec![
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 30, 0),
                    open_window_jump_return: 0.016,
                    closed_window_gap_return: -0.009,
                },
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 15, 0, 0),
                    open_window_jump_return: -0.012,
                    closed_window_gap_return: 0.004,
                },
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 16, 0, 0),
                    open_window_jump_return: 0.011,
                    closed_window_gap_return: -0.006,
                },
            ],
            detected_intraday_jump_candidates: vec![
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                    jump_return: 0.018,
                    post_shock_activity_lift: 1.35,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                    jump_return: -0.014,
                    post_shock_activity_lift: 1.10,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                    jump_return: 0.010,
                    post_shock_activity_lift: 0.95,
                },
            ],
            detected_close_open_gap_candidates: vec![
                ReturnComponentsDetectedCloseOpenGapCandidate {
                    close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                    open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                    gap_return: -0.013,
                    post_shock_activity_lift: 1.40,
                },
                ReturnComponentsDetectedCloseOpenGapCandidate {
                    close_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 21, 0, 0),
                    open_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 14, 30, 0),
                    gap_return: 0.008,
                    post_shock_activity_lift: 1.05,
                },
            ],
            nig_shared_plus_shift_candidates: vec![
                NigSharedPlusShiftConfiguration::try_new(2.30, -0.80, 0.70, 0.10, -0.20)
                    .expect("high-loss NIG configuration should validate"),
                NigSharedPlusShiftConfiguration::try_new(2.45, -0.70, 0.62, 0.08, -0.15)
                    .expect("low-loss NIG configuration should validate"),
            ],
            shared_mark_law_template: SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                    .expect("gamma severity mark configuration should validate"),
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                    0.45, 8.0, 10.0,
                )
                .expect("double-exponential base-shock configuration should validate"),
                coupled_gap_marks: CoupledGapMarkConfiguration::new(0.22),
            },
            objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights::new(
                1.0, 1.0, 1.0,
            ),
        }
    }

    fn utc_nanos(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("fixture timestamp should be valid")
            .timestamp_nanos_opt()
            .expect("fixture timestamp nanos should fit in i64")
    }

    fn calibration_stage_test_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        },
        {
          "weekday": "Tuesday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        },
        {
          "weekday": "Wednesday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        }
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
        )
        .expect("calendar registry fixture should load")
    }

    fn calibration_stage_timestamp_fixture() -> Vec<TimestampEventInputRecord> {
        vec![
            timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
            timestamp_input_record(2, 2026, 2, 3, 14, 30, 0),
            timestamp_input_record(3, 2026, 2, 3, 18, 45, 0),
            timestamp_input_record(4, 2026, 2, 4, 14, 30, 0),
            timestamp_input_record(5, 2026, 2, 4, 20, 30, 0),
        ]
    }

    fn timestamp_input_record(
        source_record_position: u64,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> TimestampEventInputRecord {
        TimestampEventInputRecord {
            source_record_position,
            timestamp_utc: Utc
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()
                .expect("test timestamp should be valid"),
        }
    }
}
