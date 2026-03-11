use super::support::*;

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

    assert!(payload.day_of_week_factor.iter().all(|value| *value == 1.0));
    assert!(payload.day_of_year_factor.iter().all(|value| *value == 1.0));
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
fn hawkes_stage_rejects_seasonality_scaled_supercritical_candidates() {
    let calendar_registry = calibration_stage_test_registry();
    let mut seasonality_payload = build_seasonality_stage_success_payload();
    seasonality_payload.intraday_factor_by_bin.fill(2.0);
    seasonality_payload.day_of_week_factor[0] = 1.6;

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

    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    match report
        .failure
        .as_ref()
        .expect("failure report should be populated")
    {
        HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
            candidate_diagnostics,
        } => {
            assert_eq!(candidate_diagnostics.len(), 1);
            assert_eq!(
                candidate_diagnostics[0].candidate_id,
                "candidate_supercritical_under_seasonality"
            );
            assert!(candidate_diagnostics[0].constraint_violation_count > 0);
            assert!(
                candidate_diagnostics[0]
                    .warnings
                    .iter()
                    .any(|warning| { warning.contains("effective branching ratio") })
            );
        }
        _ => panic!("expected seasonality-scaled stationarity failure"),
    }
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
    assert_eq!(candidate_diagnostics.len(), 2);

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
        high_loss.implied_metrics.max_runtime_hourly_seasonality_factor >= 1.0
    );
    assert!(
        (high_loss.implied_metrics.max_runtime_effective_branching_ratio
            - high_loss.implied_metrics.max_runtime_hourly_seasonality_factor
                * high_loss.implied_metrics.endogenous_branching_ratio)
            .abs()
            < 1.0e-12
    );
    assert!(
        (high_loss.implied_metrics.runtime_effective_branching_margin
            - (1.0 - high_loss.implied_metrics.max_runtime_effective_branching_ratio).max(0.0))
        .abs()
            < 1.0e-12
    );
}

#[test]
fn hawkes_stage_surfaces_unrealistic_implied_event_volume_warnings() {
    let calendar_registry = calibration_stage_test_registry();
    let mut explosive_candidate = hawkes_stage_candidate_fixture()
        .into_iter()
        .next()
        .expect("fixture should provide a candidate");
    explosive_candidate.candidate_id = "candidate_explosive_volume".to_string();
    explosive_candidate.lambda_bar = 42.0;

    let report = run_hawkes_calibration_stage(
        &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            build_seasonality_stage_success_payload(),
            vec![explosive_candidate],
        ),
    );

    let candidate = &report
        .fitted_stage_payload
        .as_ref()
        .expect("single explosive candidate should still emit diagnostics")
        .candidate_diagnostics[0];
    assert!(
        candidate
            .warnings
            .iter()
            .any(|warning| warning.contains("observation window"))
    );
    assert!(
        candidate
            .implied_metrics
            .event_volume_ratio_vs_total_observed
            > 1.5
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
fn return_components_stage_execution_succeeds_for_memory_and_disk_pipeline_handoffs() {
    let hawkes_payload = build_hawkes_stage_success_payload();
    let calendar_registry = calibration_stage_test_registry();
    let memory_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
    let disk_run_spec =
        build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

    let memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload.clone(),
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
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
        "known_calendar_observed_jump_and_session_boundary_signals_to_candidate_families.v1"
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
        .detected_intraday_jump_candidates
        .clear();

    let report = run_return_components_calibration_stage(
        &run_spec,
        &calendar_registry,
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
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let second_memory_report = run_return_components_calibration_stage(
        &memory_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
            hawkes_payload,
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let first_disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
            "artifact://hawkes/run-001",
            return_components_stage_candidate_generation_fixture(),
        ),
    );
    let second_disk_report = run_return_components_calibration_stage(
        &disk_run_spec,
        &calendar_registry,
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
