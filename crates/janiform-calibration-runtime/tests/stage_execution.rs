mod support;

use std::sync::{LazyLock, Mutex};

use janiform_market_time::business_time_clock::{
    FourierBusinessTimeClockAxis, FourierBusinessTimeClockHarmonicSeries,
};
use support::shared::{
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationRunSeedSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageExecutionStatus, CalibrationStagePayloadSource, CalibrationStageSeedOverride,
    CalibrationStageSolverSettings, CalibrationTickObjectiveDataUsagePolicy, CancellationToken,
    HawkesKernelValidationStatus, HawkesStageExecutionFailure, HawkesStageExecutionInput,
    HawkesStructuredAlphaParameterization, JointRefinementStageExecutionFailure,
    JointRefinementStageExecutionInput, JointRefinementStageFittedPayload,
    JointRefinementStageSeasonalityFactorsInput, ReturnComponentsStageExecutionFailure,
    ReturnComponentsStageExecutionInput, SeasonalityStageExecutionFailure,
    SeasonalityStageExecutionInput, SeasonalityStageNormalizedPayload, build_data_inputs,
    build_hawkes_stage_run_spec, build_hawkes_stage_success_payload,
    build_mean_reversion_stage_success_payload, build_return_components_stage_pipeline_run_spec,
    build_return_components_stage_standalone_run_spec, build_run_contract_with_mode_and_resume,
    build_seasonality_stage_run_spec, build_seasonality_stage_success_payload,
    build_solver_settings, calibration_stage_test_registry, calibration_stage_timestamp_fixture,
    hawkes_stage_candidate_fixture, hawkes_stage_constraint_violating_candidate_fixture,
    joint_refinement_stage_option_pricing_input_fixture,
    normalize_joint_refinement_runtime_profile_for_reproducibility,
    return_components_stage_candidate_generation_fixture, run_hawkes_calibration_stage,
    run_joint_refinement_calibration_stage, run_return_components_calibration_stage,
    run_seasonality_calibration_stage, timestamp_input_record,
};

const JOINT_REFINEMENT_WORKER_EXECUTABLE_ENV: &str = "JANIFORM_JOINT_REFINEMENT_WORKER_EXECUTABLE";
const JOINT_REFINEMENT_RESTART_POLICY_ENV: &str = "JANIFORM_JOINT_REFINEMENT_RESTART_POLICY";
const JOINT_REFINEMENT_RESTART_POLICY_BUDGET_DERIVED: &str = "budget_derived";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV: &str =
    "JANIFORM_JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_PROCESS_WORKER: &str = "process_worker";

static JOINT_REFINEMENT_WORKER_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn normalized_joint_refinement_payload_for_reproducibility(
    mut payload: JointRefinementStageFittedPayload,
) -> JointRefinementStageFittedPayload {
    normalize_joint_refinement_runtime_profile_for_reproducibility(
        &mut payload.optimizer_diagnostics.runtime_profile,
    );
    payload
}

fn seasonality_factors_input_from_payload(
    payload: &SeasonalityStageNormalizedPayload,
) -> JointRefinementStageSeasonalityFactorsInput {
    JointRefinementStageSeasonalityFactorsInput {
        exchange_code: payload.exchange_code.clone(),
        clock_model: payload.clock_model.clone(),
    }
}

fn set_intraday_clock_rate_bound(payload: &mut SeasonalityStageNormalizedPayload, rate_bound: f64) {
    assert!(rate_bound.is_finite());
    assert!(rate_bound > 0.0);

    set_clock_rate_bound(payload, FourierBusinessTimeClockAxis::Intraday, rate_bound);
}

fn set_annual_clock_rate_bound(payload: &mut SeasonalityStageNormalizedPayload, rate_bound: f64) {
    assert!(rate_bound.is_finite());
    assert!(rate_bound > 0.0);

    set_clock_rate_bound(payload, FourierBusinessTimeClockAxis::Annual, rate_bound);
}

fn set_clock_rate_bound(
    payload: &mut SeasonalityStageNormalizedPayload,
    axis: FourierBusinessTimeClockAxis,
    rate_bound: f64,
) {
    payload.clock_model.harmonic_series.insert(
        axis,
        FourierBusinessTimeClockHarmonicSeries {
            harmonic_order: 1,
            cosine_coefficients: vec![rate_bound.ln()],
            sine_coefficients: vec![0.0],
        },
    );
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
        first_memory_metadata.selected_harmonic_orders,
        first_disk_metadata.selected_harmonic_orders
    );
    assert_eq!(
        first_memory_metadata.evaluated_candidate_count,
        first_disk_metadata.evaluated_candidate_count
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
fn seasonality_stage_execution_neutralizes_under_supported_calendar_components() {
    let calendar_registry = calibration_stage_test_registry();
    let report = run_seasonality_calibration_stage(
        &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        SeasonalityStageExecutionInput::from_timestamp_event_records(vec![
            timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
            timestamp_input_record(2, 2026, 2, 2, 20, 30, 0),
        ]),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .normalized_stage_payload
        .as_ref()
        .expect("seasonality stage should emit a normalized payload");

    assert!(
        payload
            .clock_model
            .harmonic_orders()
            .values()
            .all(|harmonic_order| *harmonic_order == 0)
    );
    assert!(
        payload
            .business_time_rate_by_event
            .iter()
            .all(|value| *value == 1.0)
    );
}

#[test]
fn joint_refinement_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
    let _guard = JOINT_REFINEMENT_WORKER_ENV_LOCK
        .lock()
        .expect("joint-refinement worker environment lock should be available");

    let calendar_registry = calibration_stage_test_registry();
    let seasonality_payload = build_seasonality_stage_success_payload();
    let hawkes_payload = build_hawkes_stage_success_payload();
    let memory_run_spec = build_run_contract_with_mode_and_resume(
        "run-joint-stage-memory",
        CalibrationExecutionMode::Memory,
        None,
    )
    .run_spec;
    let disk_run_spec = build_run_contract_with_mode_and_resume(
        "run-joint-stage-disk",
        CalibrationExecutionMode::Disk,
        None,
    )
    .run_spec;

    let return_components_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput {
            activity_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload.clone(),
            },
            hawkes_return_source_responsibility_input: None,
            coarse_data_records: None,
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
        },
    );

    let return_components_payload = return_components_report
        .fitted_stage_payload
        .expect("return-components stage fixture should produce fitted payload");

    let memory_report = run_joint_refinement_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        JointRefinementStageExecutionInput {
            seasonality_factors_input: seasonality_factors_input_from_payload(&seasonality_payload),
            hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload.clone(),
            },
            return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: return_components_payload.clone(),
            },
            mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: build_mean_reversion_stage_success_payload(),
            },
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
            coarse_data_records: None,
            tick_objective_data_usage_policy:
                CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
            calibration_effort: None,
            restart_candidate_budget_override: Some(1),
            treat_inadmissible_calibration_warning_as_error: false,
            option_refinement_candidate_count_override: Some(1),
            option_refinement_wall_clock_budget_millis: None,
            audit_option_pricing_enabled: false,
            diagnostics: Default::default(),
            option_pricing_input: None,
        },
        CancellationToken::uncancelled(),
    );

    let disk_report = run_joint_refinement_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        JointRefinementStageExecutionInput {
            seasonality_factors_input: seasonality_factors_input_from_payload(&seasonality_payload),
            hawkes_input: CalibrationStagePayloadSource::PersistedArtifactReference {
                artifact_reference: "artifact://hawkes/run-joint-stage-disk".to_string(),
                payload: Some(hawkes_payload),
            },
            return_components_input: CalibrationStagePayloadSource::PersistedArtifactReference {
                artifact_reference: "artifact://return-components/run-joint-stage-disk".to_string(),
                payload: Some(return_components_payload),
            },
            mean_reversion_input: CalibrationStagePayloadSource::PersistedArtifactReference {
                artifact_reference: "artifact://mean-reversion/run-joint-stage-disk".to_string(),
                payload: Some(build_mean_reversion_stage_success_payload()),
            },
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
            coarse_data_records: None,
            tick_objective_data_usage_policy:
                CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
            calibration_effort: None,
            restart_candidate_budget_override: Some(1),
            treat_inadmissible_calibration_warning_as_error: false,
            option_refinement_candidate_count_override: Some(1),
            option_refinement_wall_clock_budget_millis: None,
            audit_option_pricing_enabled: false,
            diagnostics: Default::default(),
            option_pricing_input: None,
        },
        CancellationToken::uncancelled(),
    );

    assert!(memory_report.status.is_successful(), "{memory_report:#?}");
    assert!(disk_report.status.is_successful(), "{disk_report:#?}");
}

#[test]
fn joint_refinement_stage_execution_is_deterministic_across_cpu_counts() {
    with_joint_refinement_restart_policy_override(
        JOINT_REFINEMENT_RESTART_POLICY_BUDGET_DERIVED,
        || {
            let calendar_registry = calibration_stage_test_registry();

            let seasonality_payload = build_seasonality_stage_success_payload();
            let hawkes_payload = build_hawkes_stage_success_payload();
            let return_components_payload = run_return_components_calibration_stage(
                &build_run_contract_with_mode_and_resume(
                    "run-joint-stage-cpu-memory",
                    CalibrationExecutionMode::Memory,
                    None,
                )
                .run_spec,
                &calendar_registry,
                ReturnComponentsStageExecutionInput {
                    activity_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: hawkes_payload.clone(),
                    },
                    hawkes_return_source_responsibility_input: None,
                    coarse_data_records: None,
                    candidate_generation_input:
                        return_components_stage_candidate_generation_fixture(),
                },
            )
            .fitted_stage_payload
            .expect("return-components fixture should produce fitted payload");

            let memory_single_thread_report = run_joint_refinement_calibration_stage(
                &joint_refinement_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 1),
                &calendar_registry,
                JointRefinementStageExecutionInput {
                    seasonality_factors_input: seasonality_factors_input_from_payload(
                        &seasonality_payload,
                    ),
                    hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: hawkes_payload.clone(),
                    },
                    return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: return_components_payload.clone(),
                    },
                    mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: build_mean_reversion_stage_success_payload(),
                    },
                    candidate_generation_input:
                        return_components_stage_candidate_generation_fixture(),
                    coarse_data_records: None,
                    tick_objective_data_usage_policy:
                        CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
                    calibration_effort: None,
                    restart_candidate_budget_override: Some(1),
                    treat_inadmissible_calibration_warning_as_error: false,
                    option_refinement_candidate_count_override: Some(1),
                    option_refinement_wall_clock_budget_millis: None,
                    audit_option_pricing_enabled: false,
                    diagnostics: Default::default(),
                    option_pricing_input: None,
                },
                CancellationToken::uncancelled(),
            );

            let memory_multi_thread_report = run_joint_refinement_calibration_stage(
                &joint_refinement_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 4),
                &calendar_registry,
                JointRefinementStageExecutionInput {
                    seasonality_factors_input: seasonality_factors_input_from_payload(
                        &seasonality_payload,
                    ),
                    hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: hawkes_payload.clone(),
                    },
                    return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: return_components_payload.clone(),
                    },
                    mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: build_mean_reversion_stage_success_payload(),
                    },
                    candidate_generation_input:
                        return_components_stage_candidate_generation_fixture(),
                    coarse_data_records: None,
                    tick_objective_data_usage_policy:
                        CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
                    calibration_effort: None,
                    restart_candidate_budget_override: Some(1),
                    treat_inadmissible_calibration_warning_as_error: false,
                    option_refinement_candidate_count_override: Some(1),
                    option_refinement_wall_clock_budget_millis: None,
                    audit_option_pricing_enabled: false,
                    diagnostics: Default::default(),
                    option_pricing_input: None,
                },
                CancellationToken::uncancelled(),
            );

            let memory_single_thread_payload = memory_single_thread_report
                .fitted_stage_payload
                .as_ref()
                .expect("single-thread memory report should include fitted payload");
            let memory_multi_thread_payload = memory_multi_thread_report
                .fitted_stage_payload
                .as_ref()
                .expect("multi-thread memory report should include fitted payload");

            assert_eq!(
                normalized_joint_refinement_payload_for_reproducibility(
                    memory_single_thread_payload.clone()
                ),
                normalized_joint_refinement_payload_for_reproducibility(
                    memory_multi_thread_payload.clone()
                )
            );
        },
    );
}

#[test]
fn joint_refinement_stage_execution_reports_infrastructure_failure_when_worker_is_unavailable() {
    let calendar_registry = calibration_stage_test_registry();

    let seasonality_payload = build_seasonality_stage_success_payload();
    let hawkes_payload = build_hawkes_stage_success_payload();
    let return_components_payload = run_return_components_calibration_stage(
        &build_run_contract_with_mode_and_resume(
            "run-joint-stage-missing-worker",
            CalibrationExecutionMode::Memory,
            None,
        )
        .run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput {
            activity_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload.clone(),
            },
            hawkes_return_source_responsibility_input: None,
            coarse_data_records: None,
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
        },
    )
    .fitted_stage_payload
    .expect("return-components fixture should produce fitted payload");

    let run_spec = joint_refinement_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 1);
    let stage_input = JointRefinementStageExecutionInput {
        seasonality_factors_input: seasonality_factors_input_from_payload(&seasonality_payload),
        hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
            payload: hawkes_payload,
        },
        return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
            payload: return_components_payload,
        },
        mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
            payload: build_mean_reversion_stage_success_payload(),
        },
        candidate_generation_input: return_components_stage_candidate_generation_fixture(),
        coarse_data_records: None,
        tick_objective_data_usage_policy:
            CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
        calibration_effort: None,
        restart_candidate_budget_override: Some(1),
        treat_inadmissible_calibration_warning_as_error: false,
        option_refinement_candidate_count_override: Some(1),
        option_refinement_wall_clock_budget_millis: None,
        audit_option_pricing_enabled: false,
        diagnostics: Default::default(),
        option_pricing_input: None,
    };

    let report = with_joint_refinement_worker_executable_override(
        std::env::temp_dir().join("missing-janiform-joint-refinement-worker"),
        || {
            run_joint_refinement_calibration_stage(
                &run_spec,
                &calendar_registry,
                stage_input,
                CancellationToken::uncancelled(),
            )
        },
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    assert!(matches!(
        report.failure,
        Some(JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
            ref error_message,
        }) if error_message.contains("worker executable path does not exist")
    ));
}

#[test]
fn joint_refinement_stage_execution_fails_when_option_pricing_input_is_invalid() {
    let calendar_registry = calibration_stage_test_registry();
    let seasonality_payload = build_seasonality_stage_success_payload();
    let hawkes_payload = build_hawkes_stage_success_payload();
    let return_components_payload = run_return_components_calibration_stage(
        &build_run_contract_with_mode_and_resume(
            "run-joint-stage-invalid-option-pricing",
            CalibrationExecutionMode::Memory,
            None,
        )
        .run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput {
            activity_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload.clone(),
            },
            hawkes_return_source_responsibility_input: None,
            coarse_data_records: None,
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
        },
    )
    .fitted_stage_payload
    .expect("return-components fixture should produce fitted payload");

    let mut option_pricing_input = joint_refinement_stage_option_pricing_input_fixture();
    option_pricing_input.option_valuation_spot = 0.0;

    let report = run_joint_refinement_calibration_stage(
        &joint_refinement_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 1),
        &calendar_registry,
        JointRefinementStageExecutionInput {
            seasonality_factors_input: seasonality_factors_input_from_payload(&seasonality_payload),
            hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload,
            },
            return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: return_components_payload,
            },
            mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: build_mean_reversion_stage_success_payload(),
            },
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
            coarse_data_records: None,
            tick_objective_data_usage_policy:
                CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
            calibration_effort: None,
            restart_candidate_budget_override: Some(1),
            treat_inadmissible_calibration_warning_as_error: false,
            option_refinement_candidate_count_override: Some(1),
            option_refinement_wall_clock_budget_millis: None,
            audit_option_pricing_enabled: false,
            diagnostics: Default::default(),
            option_pricing_input: Some(option_pricing_input),
        },
        CancellationToken::uncancelled(),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    assert!(matches!(
        report.failure,
        Some(JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            ref error_message,
        }) if error_message.contains("strictly positive option valuation spot")
    ));
}

#[test]
fn joint_refinement_stage_execution_fails_when_restart_override_is_zero() {
    let calendar_registry = calibration_stage_test_registry();
    let seasonality_payload = build_seasonality_stage_success_payload();
    let hawkes_payload = build_hawkes_stage_success_payload();
    let return_components_payload = run_return_components_calibration_stage(
        &build_run_contract_with_mode_and_resume(
            "run-joint-stage-zero-restart-override",
            CalibrationExecutionMode::Memory,
            None,
        )
        .run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput {
            activity_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload.clone(),
            },
            hawkes_return_source_responsibility_input: None,
            coarse_data_records: None,
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
        },
    )
    .fitted_stage_payload
    .expect("return-components fixture should produce fitted payload");

    let report = run_joint_refinement_calibration_stage(
        &joint_refinement_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 1),
        &calendar_registry,
        JointRefinementStageExecutionInput {
            seasonality_factors_input: seasonality_factors_input_from_payload(&seasonality_payload),
            hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_payload,
            },
            return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: return_components_payload,
            },
            mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                payload: build_mean_reversion_stage_success_payload(),
            },
            candidate_generation_input: return_components_stage_candidate_generation_fixture(),
            coarse_data_records: None,
            tick_objective_data_usage_policy:
                CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
            calibration_effort: None,
            restart_candidate_budget_override: Some(0),
            treat_inadmissible_calibration_warning_as_error: false,
            option_refinement_candidate_count_override: Some(1),
            option_refinement_wall_clock_budget_millis: None,
            audit_option_pricing_enabled: false,
            diagnostics: Default::default(),
            option_pricing_input: None,
        },
        CancellationToken::uncancelled(),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    assert!(matches!(
        report.failure,
        Some(JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            ref error_message,
        }) if error_message.contains("restart_candidate_budget_override must be greater than zero")
    ));
}

#[test]
fn hawkes_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
    let calendar_registry = calibration_stage_test_registry();

    let seasonality_payload = build_seasonality_stage_success_payload();

    let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

    let memory_report = run_hawkes_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload.clone(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let disk_report = run_hawkes_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            seasonality_payload.clone(),
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
        disk_payload.selected_candidate_id
    );
    assert!(
        memory_payload.selected_candidate_id.contains("_refined_"),
        "hawkes stage should be able to refine beyond the coarse seed set"
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
    let calendar_registry = calibration_stage_test_registry();

    let run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);

    let report = run_hawkes_calibration_stage(
        &run_spec,
        &calendar_registry,
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
fn hawkes_stage_keeps_subcritical_candidates_stable_under_baseline_seasonality() {
    let calendar_registry = calibration_stage_test_registry();

    let mut seasonality_payload = build_seasonality_stage_success_payload();

    set_intraday_clock_rate_bound(&mut seasonality_payload, 1.6);

    let mut candidate = hawkes_stage_candidate_fixture()
        .into_iter()
        .next()
        .expect("fixture should provide a candidate");

    candidate.candidate_id = "candidate_supercritical_under_seasonality".to_string();
    candidate.beta = 1.0;
    candidate.structured_alpha = HawkesStructuredAlphaParameterization::new(0.75, 0.0, 0.0, 0.0);

    let report = run_hawkes_calibration_stage(
        &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload,
            vec![candidate],
        ),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    let fitted_stage_payload = report
        .fitted_stage_payload
        .as_ref()
        .expect("success report should include fitted payload");
    let original_candidate = fitted_stage_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == "candidate_supercritical_under_seasonality")
        .expect("candidate diagnostics should retain the coarse supercritical seed");
    let selected_candidate = fitted_stage_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == fitted_stage_payload.selected_candidate_id)
        .expect("selected candidate should be present in diagnostics");

    assert_eq!(original_candidate.constraint_violation_count, 0);
    assert!(
        original_candidate
            .implied_metrics
            .max_runtime_effective_branching_ratio
            < 1.0
    );
    assert_eq!(selected_candidate.constraint_violation_count, 0);
    assert!(
        selected_candidate
            .implied_metrics
            .max_runtime_effective_branching_ratio
            < 1.0
    );
}

#[test]
fn hawkes_stage_tracks_unobserved_runtime_seasonality_without_branching_constraint() {
    let calendar_registry = calibration_stage_test_registry();

    let mut seasonality_payload = build_seasonality_stage_success_payload();

    let annual_rate_bound = 3.0_f64;
    set_annual_clock_rate_bound(&mut seasonality_payload, annual_rate_bound);

    let mut candidate = hawkes_stage_candidate_fixture()
        .into_iter()
        .next()
        .expect("fixture should provide a candidate");

    candidate.candidate_id =
        "candidate_supercritical_under_unobserved_runtime_envelope".to_string();
    candidate.beta = 1.0;
    candidate.structured_alpha = HawkesStructuredAlphaParameterization::new(0.41, 0.0, 0.0, 0.0);

    let report = run_hawkes_calibration_stage(
        &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload,
            vec![candidate],
        ),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    let fitted_stage_payload = report
        .fitted_stage_payload
        .as_ref()
        .expect("success report should include fitted payload");
    let original_candidate = fitted_stage_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| {
            candidate.candidate_id == "candidate_supercritical_under_unobserved_runtime_envelope"
        })
        .expect("candidate diagnostics should retain the coarse supercritical seed");
    let selected_candidate = fitted_stage_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == fitted_stage_payload.selected_candidate_id)
        .expect("selected candidate should be present in diagnostics");

    assert_eq!(original_candidate.constraint_violation_count, 0);
    assert_eq!(
        original_candidate
            .implied_metrics
            .max_runtime_interval_seasonality_factor,
        annual_rate_bound.ln().exp()
    );
    assert_eq!(
        original_candidate
            .implied_metrics
            .max_runtime_effective_branching_ratio,
        original_candidate
            .implied_metrics
            .endogenous_branching_ratio
    );
    assert_eq!(selected_candidate.constraint_violation_count, 0);
    assert!(
        selected_candidate
            .implied_metrics
            .max_runtime_effective_branching_ratio
            < 1.0
    );
}

#[test]
fn hawkes_stage_objective_terms_change_with_candidate_implied_metrics() {
    let calendar_registry = calibration_stage_test_registry();

    let report = run_hawkes_calibration_stage(
        &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            build_seasonality_stage_success_payload(),
            hawkes_stage_candidate_fixture(),
        ),
    );

    let candidate_diagnostics = &report
        .fitted_stage_payload
        .as_ref()
        .expect("hawkes report should include payload")
        .candidate_diagnostics;
    assert!(
        candidate_diagnostics.len() > 2,
        "hawkes stage should evaluate refined candidates beyond the coarse seeds"
    );

    let high_loss = candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == "candidate_high_loss")
        .expect("fixture should contain candidate_high_loss");
    let low_loss = candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == "candidate_stable_low_loss")
        .expect("fixture should contain candidate_stable_low_loss");

    assert_ne!(high_loss.raw_objective_value, low_loss.raw_objective_value);
    assert_ne!(
        high_loss
            .implied_metrics
            .model_implied_open_event_rate_per_hour,
        low_loss
            .implied_metrics
            .model_implied_open_event_rate_per_hour
    );
    assert!(
        high_loss
            .objective_terms
            .iter()
            .any(|term| term.term_id == "open_session_event_rate")
    );
    assert!(
        low_loss
            .objective_terms
            .iter()
            .any(|term| term.term_id == "directional_imbalance_magnitude")
    );
    assert!(high_loss.implied_metrics.observed_open_event_rate_per_hour > 0.0);
    assert!(
        high_loss
            .implied_metrics
            .deseasonalized_open_event_rate_per_hour_target
            > 0.0
    );
    assert!(high_loss.implied_metrics.lambda_bar_scaling_ratio_applied > 0.0);
    assert!(
        high_loss
            .implied_metrics
            .max_runtime_interval_seasonality_factor
            >= 1.0
    );
    assert!(
        (high_loss
            .implied_metrics
            .max_runtime_effective_branching_ratio
            - high_loss.implied_metrics.endogenous_branching_ratio)
            .abs()
            < 1.0e-12
    );
    assert!(
        (high_loss.implied_metrics.runtime_effective_branching_margin
            - (1.0
                - high_loss
                    .implied_metrics
                    .max_runtime_effective_branching_ratio)
                .max(0.0_f64))
        .abs()
            < 1.0e-12
    );
}

#[test]
fn hawkes_stage_execution_is_deterministic_under_fixed_seed_and_solver_settings() {
    let calendar_registry = calibration_stage_test_registry();

    let seasonality_payload = build_seasonality_stage_success_payload();

    let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

    let first_memory_report = run_hawkes_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload.clone(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let second_memory_report = run_hawkes_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload,
            hawkes_stage_candidate_fixture(),
        ),
    );
    let first_disk_report = run_hawkes_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            build_seasonality_stage_success_payload(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let second_disk_report = run_hawkes_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            build_seasonality_stage_success_payload(),
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
fn hawkes_stage_execution_is_deterministic_across_cpu_counts() {
    let calendar_registry = calibration_stage_test_registry();

    let seasonality_payload = build_seasonality_stage_success_payload();

    let memory_single_thread_report = run_hawkes_calibration_stage(
        &hawkes_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 1),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload.clone(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let memory_multi_thread_report = run_hawkes_calibration_stage(
        &hawkes_run_spec_with_cpu_count(CalibrationExecutionMode::Memory, 4),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload.clone(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let disk_single_thread_report = run_hawkes_calibration_stage(
        &hawkes_run_spec_with_cpu_count(CalibrationExecutionMode::Disk, 1),
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            seasonality_payload.clone(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let disk_multi_thread_report = run_hawkes_calibration_stage(
        &hawkes_run_spec_with_cpu_count(CalibrationExecutionMode::Disk, 4),
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            seasonality_payload,
            hawkes_stage_candidate_fixture(),
        ),
    );

    let memory_single_thread_payload = memory_single_thread_report
        .fitted_stage_payload
        .as_ref()
        .expect("single-thread memory report should include fitted payload");
    let memory_multi_thread_payload = memory_multi_thread_report
        .fitted_stage_payload
        .as_ref()
        .expect("multi-thread memory report should include fitted payload");
    let disk_single_thread_payload = disk_single_thread_report
        .fitted_stage_payload
        .as_ref()
        .expect("single-thread disk report should include fitted payload");
    let disk_multi_thread_payload = disk_multi_thread_report
        .fitted_stage_payload
        .as_ref()
        .expect("multi-thread disk report should include fitted payload");

    assert_eq!(memory_single_thread_payload, memory_multi_thread_payload);
    assert_eq!(disk_single_thread_payload, disk_multi_thread_payload);
    assert_eq!(
        memory_single_thread_payload.selected_candidate_id,
        disk_multi_thread_payload.selected_candidate_id
    );
    assert_eq!(
        memory_single_thread_payload.fitted_parameters,
        disk_multi_thread_payload.fitted_parameters
    );
    assert_eq!(
        memory_single_thread_payload.candidate_diagnostics,
        disk_multi_thread_payload.candidate_diagnostics
    );
}

#[test]
fn return_components_stage_execution_succeeds_for_memory_and_disk_pipeline_handoffs() {
    let calendar_registry = calibration_stage_test_registry();

    let hawkes_payload = build_hawkes_stage_success_payload();

    let memory_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

    let memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload.clone(),
            None,
            return_components_stage_candidate_generation_fixture(),
        ),
    );

    let disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference_with_payload(
            "artifact://hawkes/run-001",
            Some(hawkes_payload.clone()),
            None,
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
        "continuous_distribution_surprise_arrival_jump_gap_activity_and_price_sanity_bucketed_weighted_terms.v1"
    );
    assert_eq!(
        memory_payload.candidate_generation_policy,
        "observed_open_session_returns_and_shock_evidence_to_explicit_fitted_return_component_sublayers.v1"
    );

    for candidate_diagnostic in &memory_payload.candidate_diagnostics {
        let term_ids = candidate_diagnostic
            .objective_terms
            .iter()
            .map(|objective_term| objective_term.term_id.as_str())
            .collect::<Vec<_>>();

        assert!(term_ids.contains(&"continuous_distribution_fit"));
        assert!(term_ids.contains(&"surprise_arrival_fit"));
        assert!(term_ids.contains(&"open_jump_fit"));
        assert!(term_ids.contains(&"closed_gap_fit"));
        assert!(term_ids.contains(&"activity_coupling_fit"));
        assert!(term_ids.contains(&"price_range_sanity_fit"));
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
                .continuous_distribution_fit
                .is_finite()
        );
        assert!(
            candidate_diagnostic
                .objective_metric_buckets
                .surprise_arrival_fit
                .is_finite()
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
        assert!(
            candidate_diagnostic
                .objective_metric_buckets
                .price_range_sanity_fit
                .is_finite()
        );
    }

    let memory_metadata = memory_report
        .metadata
        .as_ref()
        .expect("memory report should include metadata");

    assert!(memory_metadata.input_candidate_count > 0);
    assert!(memory_metadata.input_candidate_count <= 10_000);
    assert!(memory_metadata.valid_candidate_count <= memory_metadata.input_candidate_count);
}

fn hawkes_run_spec_with_cpu_count(
    execution_mode: CalibrationExecutionMode,
    cpu_count: usize,
) -> CalibrationRunSpec {
    let mut run_spec = build_hawkes_stage_run_spec(execution_mode);

    let hawkes_solver_settings = run_spec
        .optimizer_settings_by_stage
        .iter_mut()
        .find(|stage_solver_settings| stage_solver_settings.stage == CalibrationStage::Hawkes)
        .expect("hawkes stage solver settings should exist");

    hawkes_solver_settings.optimizer_settings.cpu_count = cpu_count;

    run_spec.validate().expect("run spec should remain valid");

    run_spec
}

fn joint_refinement_run_spec_with_cpu_count(
    execution_mode: CalibrationExecutionMode,
    cpu_count: usize,
) -> CalibrationRunSpec {
    let mut run_spec =
        build_run_contract_with_mode_and_resume("run-joint-stage-cpu-count", execution_mode, None)
            .run_spec;

    let joint_refinement_solver_settings = run_spec
        .optimizer_settings_by_stage
        .iter_mut()
        .find(|stage_solver_settings| {
            stage_solver_settings.stage == CalibrationStage::JointRefinement
        })
        .expect("joint-refinement stage solver settings should exist");

    joint_refinement_solver_settings
        .optimizer_settings
        .cpu_count = cpu_count;

    run_spec.validate().expect("run spec should remain valid");

    run_spec
}

/// Never use this in production paths!
fn with_joint_refinement_worker_executable_override<T>(
    worker_executable: std::path::PathBuf,
    operation: impl FnOnce() -> T,
) -> T {
    let _guard = JOINT_REFINEMENT_WORKER_ENV_LOCK
        .lock()
        .expect("joint-refinement worker environment lock should be available");

    let previous_value = std::env::var_os(JOINT_REFINEMENT_WORKER_EXECUTABLE_ENV);
    let previous_backend_value = std::env::var_os(JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV);

    // SAFETY: This helper serializes environment mutation through a process-local mutex and
    // restores the previous value before releasing the lock.
    unsafe {
        std::env::set_var(JOINT_REFINEMENT_WORKER_EXECUTABLE_ENV, &worker_executable);
        std::env::set_var(
            JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV,
            JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_PROCESS_WORKER,
        );
    }

    let result = operation();

    if let Some(previous_value) = previous_value {
        // SAFETY: The same mutex guard is still held while restoring the previous value.
        unsafe {
            std::env::set_var(JOINT_REFINEMENT_WORKER_EXECUTABLE_ENV, previous_value);
        }
    } else {
        // SAFETY: The same mutex guard is still held while clearing the temporary override.
        unsafe {
            std::env::remove_var(JOINT_REFINEMENT_WORKER_EXECUTABLE_ENV);
        }
    }

    if let Some(previous_backend_value) = previous_backend_value {
        // SAFETY: The same mutex guard is still held while restoring the previous value.
        unsafe {
            std::env::set_var(
                JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV,
                previous_backend_value,
            );
        }
    } else {
        // SAFETY: The same mutex guard is still held while clearing the temporary override.
        unsafe {
            std::env::remove_var(JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV);
        }
    }

    result
}

/// Never use this in production paths!
fn with_joint_refinement_restart_policy_override<T>(
    restart_policy: &str,
    operation: impl FnOnce() -> T,
) -> T {
    let _guard = JOINT_REFINEMENT_WORKER_ENV_LOCK
        .lock()
        .expect("joint-refinement worker environment lock should be available");
    let previous_value = std::env::var_os(JOINT_REFINEMENT_RESTART_POLICY_ENV);

    // SAFETY: This helper serializes environment mutation through a process-local mutex and
    // restores the previous value before releasing the lock.
    unsafe {
        std::env::set_var(JOINT_REFINEMENT_RESTART_POLICY_ENV, restart_policy);
    }

    let result = operation();

    if let Some(previous_value) = previous_value {
        // SAFETY: The same mutex guard is still held while restoring the previous value.
        unsafe {
            std::env::set_var(JOINT_REFINEMENT_RESTART_POLICY_ENV, previous_value);
        }
    } else {
        // SAFETY: The same mutex guard is still held while clearing the temporary override.
        unsafe {
            std::env::remove_var(JOINT_REFINEMENT_RESTART_POLICY_ENV);
        }
    }

    result
}

#[test]
fn return_components_stage_execution_supports_standalone_runs_in_memory_and_disk_modes() {
    let hawkes_payload = build_hawkes_stage_success_payload();
    let calendar_registry = calibration_stage_test_registry();

    let memory_run_spec =
        build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec =
        build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Disk);

    let memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload,
            None,
            return_components_stage_candidate_generation_fixture(),
        ),
    );

    let disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
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
    let calendar_registry = calibration_stage_test_registry();

    let run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);

    let mut candidate_generation_input = return_components_stage_candidate_generation_fixture();
    candidate_generation_input
        .observed_open_session_returns
        .clear();

    let report = run_return_components_calibration_stage(
        &run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload,
            None,
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
    let calendar_registry = calibration_stage_test_registry();

    let memory_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
    let memory_failure_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
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
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload,
            None,
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
    let calendar_registry = calibration_stage_test_registry();

    let memory_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

    let first_memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload.clone(),
            None,
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let second_memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload,
            None,
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let first_disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference_with_payload(
            "artifact://hawkes/run-001",
            Some(build_hawkes_stage_success_payload()),
            None,
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let second_disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference_with_payload(
            "artifact://hawkes/run-001",
            Some(build_hawkes_stage_success_payload()),
            None,
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
