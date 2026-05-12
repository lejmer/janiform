use serde::Serialize;

pub(crate) use crate::stages::seasonality_runtime::{
    RuntimeHourlySeasonalityFactorsBuildInput,
    build_runtime_hourly_seasonality_factors_from_components,
    maximum_realizable_runtime_hourly_seasonality_factor_from_components,
    maximum_runtime_hourly_seasonality_factor_from_payload,
};
use janiform_calibration_artifacts::{
    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION, CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    SeasonalityStageNormalizationMetadata, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, TimestampEventInputRecord, TimestampEventSessionFilter,
};
use janiform_market_time::seasonality::{
    DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimatorSettings,
    DeterministicSeasonalityCompositionInput, HourOfWeekSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings, ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsReportOutput, compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
    estimate_hour_of_week_seasonality, estimate_intraday_seasonality,
    generate_seasonality_diagnostics_report,
};
/// Configurable knobs for deterministic seasonality stage execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeasonalityStageExecutionSettings {
    pub session_filter: TimestampEventSessionFilter,
    pub sparse_hour_event_threshold: usize,
    pub intraday_settings: IntradaySeasonalityEstimatorSettings,
    pub day_of_week_settings: DayOfWeekSeasonalityEstimatorSettings,
    pub day_of_year_settings: DayOfYearSeasonalityEstimatorSettings,
}

impl Default for SeasonalityStageExecutionSettings {
    fn default() -> Self {
        Self {
            session_filter: TimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            intraday_settings: IntradaySeasonalityEstimatorSettings::default(),
            day_of_week_settings: DayOfWeekSeasonalityEstimatorSettings::default(),
            day_of_year_settings: DayOfYearSeasonalityEstimatorSettings::default(),
        }
    }
}

const MINIMUM_DISTINCT_WEEKDAYS_FOR_DAY_OF_WEEK_COMPONENT: usize = 4;
const MINIMUM_DISTINCT_DAYS_OF_YEAR_FOR_DAY_OF_YEAR_COMPONENT: usize = 90;

/// Input payload for standalone or pipeline-integrated seasonality-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityStageExecutionInput {
    pub timestamp_event_records: Vec<TimestampEventInputRecord>,
    pub scheduled_event_overlays: Vec<ScheduledEventOverlayDefinition>,
    pub settings: SeasonalityStageExecutionSettings,
    pub dataset_id_override: Option<String>,
    pub estimator_version_override: Option<String>,
}

impl SeasonalityStageExecutionInput {
    pub fn from_timestamp_event_records(
        timestamp_event_records: Vec<TimestampEventInputRecord>,
    ) -> Self {
        Self {
            timestamp_event_records,
            scheduled_event_overlays: Vec::new(),
            settings: SeasonalityStageExecutionSettings::default(),
            dataset_id_override: None,
            estimator_version_override: None,
        }
    }
}

/// Success and failure status for stage execution reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageExecutionStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageOverlayMetadata {
    pub overlays_enabled: bool,
    pub configured_overlay_count: usize,
    pub enabled_overlay_count: usize,
    pub active_overlay_event_count: usize,
    pub precedence_policy: String,
    pub merge_policy: String,
    pub window_boundary_policy: String,
    pub configured_overlay_ids_in_order: Vec<String>,
    pub applied_overlay_ids_in_order: Vec<String>,
    pub overlay_source_labels_in_order: Vec<String>,
}

/// Stable seasonality-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageNormalizedPayload {
    pub schema_version: &'static str,
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub intraday_estimate_schema_version: String,
    pub day_of_week_estimate_schema_version: String,
    pub hour_of_week_estimate_schema_version: String,
    pub day_of_year_estimate_schema_version: String,
    pub composition_schema_version: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_record_positions: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timestamp_utc_nanos: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exchange_local_minute_of_day: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exchange_local_day_of_week: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exchange_local_day_of_year: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub market_state_codes: Vec<u8>,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub hour_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub normalized_combined_log_component_by_event: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub normalized_combined_factor_by_event: Vec<f64>,
    pub normalization_metadata: SeasonalityStageNormalizationMetadata,
    pub overlay_metadata: SeasonalityStageOverlayMetadata,
    pub diagnostics_report: SeasonalityDiagnosticsReportOutput,
}

impl SeasonalityStageNormalizedPayload {
    pub(crate) fn without_event_level_vectors(&self) -> Self {
        Self {
            schema_version: self.schema_version,
            dataset_id: self.dataset_id.clone(),
            exchange_code: self.exchange_code.clone(),
            trade_events_data_reference: self.trade_events_data_reference.clone(),
            timestamp_event_input_schema_version: self.timestamp_event_input_schema_version.clone(),
            timestamp_event_preprocessed_schema_version: self
                .timestamp_event_preprocessed_schema_version
                .clone(),
            intraday_estimate_schema_version: self.intraday_estimate_schema_version.clone(),
            day_of_week_estimate_schema_version: self.day_of_week_estimate_schema_version.clone(),
            hour_of_week_estimate_schema_version: self.hour_of_week_estimate_schema_version.clone(),
            day_of_year_estimate_schema_version: self.day_of_year_estimate_schema_version.clone(),
            composition_schema_version: self.composition_schema_version.clone(),
            source_record_positions: Vec::new(),
            timestamp_utc_nanos: Vec::new(),
            exchange_local_minute_of_day: Vec::new(),
            exchange_local_day_of_week: Vec::new(),
            exchange_local_day_of_year: Vec::new(),
            market_state_codes: Vec::new(),
            intraday_minutes_per_bin: self.intraday_minutes_per_bin,
            intraday_factor_by_bin: self.intraday_factor_by_bin.clone(),
            day_of_week_factor: self.day_of_week_factor.clone(),
            hour_of_week_factor: self.hour_of_week_factor.clone(),
            day_of_year_factor: self.day_of_year_factor.clone(),
            normalized_combined_log_component_by_event: Vec::new(),
            normalized_combined_factor_by_event: Vec::new(),
            normalization_metadata: self.normalization_metadata.clone(),
            overlay_metadata: self.overlay_metadata.clone(),
            diagnostics_report: self.diagnostics_report.clone(),
        }
    }
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub dataset_id: String,
    pub estimator_version: String,
    pub input_record_count: usize,
    pub accepted_record_count: usize,
    pub filtered_record_count: usize,
    pub diagnostics_report_schema_version: String,
    pub diagnostics_snapshot_fingerprint_algorithm: String,
    pub diagnostics_snapshot_fingerprint_hex: String,
    pub normalization_policy: String,
}

/// Structured failure modes for seasonality-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum SeasonalityStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingTimestampEventRecords,
    EmptyDatasetId,
    EmptyEstimatorVersion,
    NoAcceptedTimestampEventsAfterPreprocessing,
    TimestampPreprocessingFailed { error_message: String },
    IntradayEstimationFailed { error_message: String },
    DayOfWeekEstimationFailed { error_message: String },
    HourOfWeekEstimationFailed { error_message: String },
    DayOfYearEstimationFailed { error_message: String },
    DeterministicCompositionFailed { error_message: String },
    DiagnosticsReportFailed { error_message: String },
}

/// Structured seasonality-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<SeasonalityStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_stage_payload: Option<SeasonalityStageNormalizedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SeasonalityStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<SeasonalityStageExecutionFailure>,
}

/// Execute the deterministic seasonality stage and emit normalized handoff-ready output.
pub fn run_seasonality_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: SeasonalityStageExecutionInput,
) -> SeasonalityStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Seasonality)
    {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Seasonality)
    else {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let SeasonalityStageExecutionInput {
        timestamp_event_records,
        scheduled_event_overlays,
        settings,
        dataset_id_override,
        estimator_version_override,
    } = stage_input;

    if timestamp_event_records.is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingTimestampEventRecords,
        );
    }

    let input_record_count = timestamp_event_records.len();
    let resolved_dataset_id = dataset_id_override
        .unwrap_or_else(|| run_spec.data_inputs.trade_events_data_reference.clone());
    if resolved_dataset_id.trim().is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyDatasetId,
        );
    }

    let resolved_estimator_version = estimator_version_override
        .unwrap_or_else(|| CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string());
    if resolved_estimator_version.trim().is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyEstimatorVersion,
        );
    }

    let preprocessed = match calendar_registry.preprocess_timestamp_events(
        &run_spec.data_inputs.exchange_code,
        timestamp_event_records,
        settings.session_filter,
        settings.sparse_hour_event_threshold,
    ) {
        Ok(preprocessed) => preprocessed,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::TimestampPreprocessingFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    if preprocessed.timestamp_utc_nanos.is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::NoAcceptedTimestampEventsAfterPreprocessing,
        );
    }

    let intraday_estimate = match estimate_intraday_seasonality(
        &preprocessed.exchange_local_minute_of_day,
        settings.intraday_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::IntradayEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let day_of_week_estimate = match estimate_day_of_week_seasonality(
        &preprocessed.exchange_local_day_of_week,
        settings.day_of_week_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DayOfWeekEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };
    let day_of_week_estimate = neutralize_day_of_week_component_when_under_supported(
        day_of_week_estimate,
        &preprocessed.exchange_local_day_of_week,
    );

    let hour_of_week_estimate = match estimate_hour_of_week_seasonality(
        &preprocessed.exchange_local_day_of_week,
        &preprocessed.exchange_local_minute_of_day,
        HourOfWeekSeasonalityEstimatorSettings {
            minimum_bucket_events: settings.day_of_week_settings.minimum_bucket_events,
            pseudocount: settings.day_of_week_settings.pseudocount,
        },
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::HourOfWeekEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let day_of_year_estimate = match estimate_day_of_year_seasonality(
        &preprocessed.exchange_local_day_of_year,
        settings.day_of_year_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DayOfYearEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };
    let day_of_year_estimate = neutralize_day_of_year_component_when_under_supported(
        day_of_year_estimate,
        &preprocessed.exchange_local_day_of_year,
    );

    let composition =
        match compose_deterministic_seasonality_signal(DeterministicSeasonalityCompositionInput {
            timestamp_utc_nanos: &preprocessed.timestamp_utc_nanos,
            exchange_local_minute_of_day: &preprocessed.exchange_local_minute_of_day,
            exchange_local_day_of_week: &preprocessed.exchange_local_day_of_week,
            exchange_local_day_of_year: &preprocessed.exchange_local_day_of_year,
            intraday_minutes_per_bin: intraday_estimate.minutes_per_bin,
            intraday_log_component_by_bin: &intraday_estimate.intraday_log_component_by_bin,
            day_of_week_log_component: &day_of_week_estimate.day_of_week_log_component,
            day_of_year_log_component: &day_of_year_estimate.day_of_year_log_component,
            scheduled_event_overlays: &scheduled_event_overlays,
        }) {
            Ok(composition) => composition,
            Err(source) => {
                return build_seasonality_stage_failure_report(
                    run_spec,
                    SeasonalityStageExecutionFailure::DeterministicCompositionFailed {
                        error_message: source.to_string(),
                    },
                );
            }
        };

    let diagnostics_report = match generate_seasonality_diagnostics_report(
        janiform_market_time::seasonality::SeasonalityDiagnosticsReportInput {
            dataset_id: &resolved_dataset_id,
            input_schema_version: preprocessed.input_schema_version,
            preprocessed_schema_version: preprocessed.schema_version,
            exchange_code: &preprocessed.exchange_code,
            exchange_timezone_name: &preprocessed.exchange_timezone_name,
            source_record_positions: &preprocessed.source_record_positions,
            timestamp_utc_nanos: &preprocessed.timestamp_utc_nanos,
            exchange_local_minute_of_day: &preprocessed.exchange_local_minute_of_day,
            exchange_local_day_of_week: &preprocessed.exchange_local_day_of_week,
            exchange_local_day_of_year: &preprocessed.exchange_local_day_of_year,
            market_state_codes: &preprocessed.market_state_codes,
            intraday_estimate: &intraday_estimate,
            day_of_week_estimate: &day_of_week_estimate,
            day_of_year_estimate: &day_of_year_estimate,
            composition: &composition,
            estimator_version: &resolved_estimator_version,
        },
    ) {
        Ok(report) => report,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DiagnosticsReportFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let normalization_metadata = SeasonalityStageNormalizationMetadata {
        normalization_policy: composition
            .diagnostics
            .normalization_metadata
            .normalization_policy
            .to_string(),
        fit_window_event_count: composition
            .diagnostics
            .normalization_metadata
            .fit_window_event_count,
        target_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .target_mean_factor,
        pre_normalization_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .pre_normalization_mean_factor,
        normalization_constant: composition
            .diagnostics
            .normalization_metadata
            .normalization_constant,
        normalization_log_offset: composition
            .diagnostics
            .normalization_metadata
            .normalization_log_offset,
        post_normalization_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .post_normalization_mean_factor,
        mean_anchor_tolerance: composition
            .diagnostics
            .normalization_metadata
            .mean_anchor_tolerance,
    };

    let overlay_metadata = SeasonalityStageOverlayMetadata {
        overlays_enabled: composition
            .diagnostics
            .overlay_provenance_metadata
            .overlays_enabled,
        configured_overlay_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_count,
        enabled_overlay_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .enabled_overlay_count,
        active_overlay_event_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .active_overlay_event_count,
        precedence_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .precedence_policy
            .to_string(),
        merge_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .merge_policy
            .to_string(),
        window_boundary_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .window_boundary_policy
            .to_string(),
        configured_overlay_ids_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_ids_in_order
            .clone(),
        applied_overlay_ids_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .applied_overlay_ids_in_order
            .clone(),
        overlay_source_labels_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .overlay_source_labels_in_order
            .clone(),
    };

    let normalized_stage_payload = SeasonalityStageNormalizedPayload {
        schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
        dataset_id: resolved_dataset_id.clone(),
        exchange_code: preprocessed.exchange_code.clone(),
        trade_events_data_reference: run_spec.data_inputs.trade_events_data_reference.clone(),
        timestamp_event_input_schema_version: preprocessed.input_schema_version.to_string(),
        timestamp_event_preprocessed_schema_version: preprocessed.schema_version.to_string(),
        intraday_estimate_schema_version: intraday_estimate.schema_version.to_string(),
        day_of_week_estimate_schema_version: day_of_week_estimate.schema_version.to_string(),
        hour_of_week_estimate_schema_version: hour_of_week_estimate.schema_version.to_string(),
        day_of_year_estimate_schema_version: day_of_year_estimate.schema_version.to_string(),
        composition_schema_version: composition.schema_version.to_string(),
        source_record_positions: preprocessed.source_record_positions.clone(),
        timestamp_utc_nanos: preprocessed.timestamp_utc_nanos.clone(),
        exchange_local_minute_of_day: preprocessed.exchange_local_minute_of_day.clone(),
        exchange_local_day_of_week: preprocessed.exchange_local_day_of_week.clone(),
        exchange_local_day_of_year: preprocessed.exchange_local_day_of_year.clone(),
        market_state_codes: preprocessed.market_state_codes.clone(),
        intraday_minutes_per_bin: intraday_estimate.minutes_per_bin,
        intraday_factor_by_bin: intraday_estimate.intraday_factor_by_bin.clone(),
        day_of_week_factor: day_of_week_estimate.day_of_week_factor.clone(),
        hour_of_week_factor: hour_of_week_estimate.hour_of_week_factor.clone(),
        day_of_year_factor: day_of_year_estimate.day_of_year_factor.clone(),
        normalized_combined_log_component_by_event: composition
            .combined_log_component_by_event
            .clone(),
        normalized_combined_factor_by_event: composition.combined_factor_by_event.clone(),
        normalization_metadata: normalization_metadata.clone(),
        overlay_metadata,
        diagnostics_report: diagnostics_report.clone(),
    };

    let handoffs = build_seasonality_stage_handoff_outputs(run_spec);

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::Seasonality);

    SeasonalityStageExecutionReport {
        schema_version: CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Seasonality,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
        handoffs,
        normalized_stage_payload: Some(normalized_stage_payload),
        metadata: Some(SeasonalityStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings,
            dataset_id: resolved_dataset_id,
            estimator_version: resolved_estimator_version,
            input_record_count,
            accepted_record_count: preprocessed.diagnostics.accepted_record_count,
            filtered_record_count: preprocessed.diagnostics.filtered_record_count,
            diagnostics_report_schema_version: diagnostics_report.schema_version.to_string(),
            diagnostics_snapshot_fingerprint_algorithm: diagnostics_report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_algorithm
                .to_string(),
            diagnostics_snapshot_fingerprint_hex: diagnostics_report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_hex
                .clone(),
            normalization_policy: normalization_metadata.normalization_policy,
        }),
        failure: None,
    }
}

fn build_seasonality_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<SeasonalityStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Seasonality)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => SeasonalityStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport(),
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                SeasonalityStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference().map(str::to_string),
                }
            }
        })
        .collect()
}
fn build_seasonality_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: SeasonalityStageExecutionFailure,
) -> SeasonalityStageExecutionReport {
    SeasonalityStageExecutionReport {
        schema_version: CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Seasonality,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        normalized_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

fn neutralize_day_of_week_component_when_under_supported(
    estimate: DayOfWeekSeasonalityEstimateOutput,
    observed_day_of_week: &[u8],
) -> DayOfWeekSeasonalityEstimateOutput {
    if distinct_u8_bucket_count(observed_day_of_week)
        >= MINIMUM_DISTINCT_WEEKDAYS_FOR_DAY_OF_WEEK_COMPONENT
    {
        return estimate;
    }

    let bucket_count = estimate.day_of_week_factor.len();

    DayOfWeekSeasonalityEstimateOutput {
        day_of_week_log_component: vec![0.0; bucket_count],
        day_of_week_factor: vec![1.0; bucket_count],
        fallback_bucket_flags: vec![1; bucket_count],
        diagnostics: janiform_market_time::seasonality::DayOfWeekSeasonalityDiagnostics {
            fallback_bucket_count: bucket_count,
            ..estimate.diagnostics
        },
        ..estimate
    }
}

fn neutralize_day_of_year_component_when_under_supported(
    estimate: DayOfYearSeasonalityEstimateOutput,
    observed_day_of_year: &[u16],
) -> DayOfYearSeasonalityEstimateOutput {
    if distinct_u16_bucket_count(observed_day_of_year)
        >= MINIMUM_DISTINCT_DAYS_OF_YEAR_FOR_DAY_OF_YEAR_COMPONENT
    {
        return estimate;
    }

    let bucket_count = estimate.day_of_year_factor.len();

    DayOfYearSeasonalityEstimateOutput {
        day_of_year_log_component: vec![0.0; bucket_count],
        day_of_year_factor: vec![1.0; bucket_count],
        fallback_bucket_flags: vec![1; bucket_count],
        diagnostics: janiform_market_time::seasonality::DayOfYearSeasonalityDiagnostics {
            fallback_bucket_count: bucket_count,
            ..estimate.diagnostics
        },
        ..estimate
    }
}

fn distinct_u8_bucket_count(values: &[u8]) -> usize {
    let mut bucket_seen = [false; 256];
    let mut distinct_bucket_count = 0_usize;

    for &value in values {
        let bucket_index = value as usize;
        if !bucket_seen[bucket_index] {
            bucket_seen[bucket_index] = true;
            distinct_bucket_count += 1;
        }
    }

    distinct_bucket_count
}

fn distinct_u16_bucket_count(values: &[u16]) -> usize {
    let mut bucket_seen = [false; 367];
    let mut distinct_bucket_count = 0_usize;

    for &value in values {
        let bucket_index = value as usize;
        if !bucket_seen[bucket_index] {
            bucket_seen[bucket_index] = true;
            distinct_bucket_count += 1;
        }
    }

    distinct_bucket_count
}
