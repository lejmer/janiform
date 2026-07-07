use std::collections::BTreeMap;
use std::panic;
use std::sync::mpsc::{self, Sender};

use chrono_tz::Tz;
use serde::Serialize;

mod policy;
mod progress;
mod runtime;

use policy::resolve_clock_fit_settings;
use progress::{emit_seasonality_progress, forward_clock_fit_progress};
pub(crate) use runtime::{
    RuntimeIntervalSeasonalityFactorsBuildInput,
    build_runtime_interval_seasonality_factors_from_components,
    maximum_runtime_interval_seasonality_factor_from_payload,
};
use runtime::{
    exchange_local_minute_of_week, materialize_event_rates, materialize_open_event_gap_exposures,
    parse_preprocessed_timezone,
};

use crate::stages::progress::StageProgressUpdate;
use janiform_calibration_artifacts::{
    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION, CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationResolvedStageSeed,
    CalibrationRunSpec, CalibrationStage, CalibrationStageHandoffTransport,
    CalibrationStageSeedSource, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_foundations::cancellation::CancellationToken;
pub use janiform_market_time::business_time_clock::FourierBusinessTimeClockModel;
use janiform_market_time::business_time_clock::{
    FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION, FourierBusinessTimeClockAxis,
    FourierBusinessTimeClockFitInput, FourierBusinessTimeClockFitProgress,
    FourierBusinessTimeClockFitSummary, fit_fourier_business_time_clock_model,
};
use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, TimestampEventInputRecord, TimestampEventPreprocessingOutput,
    TimestampEventSessionFilter,
};

/// Configurable knobs for business-time clock stage execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonalityStageExecutionSettings {
    pub session_filter: TimestampEventSessionFilter,
    pub sparse_hour_event_threshold: usize,
    pub maximum_harmonic_orders: BTreeMap<FourierBusinessTimeClockAxis, usize>,
}

impl Default for SeasonalityStageExecutionSettings {
    fn default() -> Self {
        Self {
            session_filter: TimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            maximum_harmonic_orders: BTreeMap::new(),
        }
    }
}

/// Input payload for standalone or pipeline-integrated seasonality-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityStageExecutionInput {
    pub timestamp_event_records: Vec<TimestampEventInputRecord>,
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
    SuccessWithWarnings,
    Cancelled,
    Failure,
}

impl CalibrationStageExecutionStatus {
    pub const fn is_successful(self) -> bool {
        matches!(self, Self::Success | Self::SuccessWithWarnings)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeasonalityStageProgressPhase {
    InputPreprocessing,
    ClockFitPreparation,
    ClockCandidateEvaluation,
    EventRateMaterialization,
    ReportAssembly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityStageProgressUpdate {
    pub phase: SeasonalityStageProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub aggregate_progress_percent: Option<usize>,
    pub current_item_label: Option<String>,
    pub current_evaluation_count: Option<usize>,
    pub total_evaluation_budget: Option<usize>,
    pub evaluation_progress_percent: Option<usize>,
    pub current_objective_value: Option<f64>,
    pub best_objective_value: Option<f64>,
}

impl SeasonalityStageProgressUpdate {
    pub(crate) fn new(
        phase: SeasonalityStageProgressPhase,
        completed_units: usize,
        total_units: usize,
    ) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
            aggregate_progress_percent: None,
            current_item_label: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
            current_objective_value: None,
            best_objective_value: None,
        }
    }

    pub(crate) fn stage_progress_percent(&self) -> usize {
        StageProgressUpdate {
            phase: self.phase,
            completed_units: self.completed_units,
            total_units: self.total_units,
            aggregate_progress_percent: self.aggregate_progress_percent,
            current_item_label: self.current_item_label.clone(),
            current_evaluation_count: self.current_evaluation_count,
            total_evaluation_budget: self.total_evaluation_budget,
            evaluation_progress_percent: self.evaluation_progress_percent,
        }
        .stage_progress_percent()
    }
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
    pub clock_model_schema_version: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_record_positions: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timestamp_utc_nanos: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exchange_local_minute_of_week: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub market_state_codes: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub business_time_rate_by_event: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_event_business_time_delta_exposure_hours_by_event: Vec<f64>,
    pub clock_model: FourierBusinessTimeClockModel,
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
            clock_model_schema_version: self.clock_model_schema_version.clone(),
            source_record_positions: Vec::new(),
            timestamp_utc_nanos: Vec::new(),
            exchange_local_minute_of_week: Vec::new(),
            market_state_codes: Vec::new(),
            business_time_rate_by_event: Vec::new(),
            open_event_business_time_delta_exposure_hours_by_event: Vec::new(),
            clock_model: self.clock_model.clone(),
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
    pub selected_harmonic_orders: BTreeMap<FourierBusinessTimeClockAxis, usize>,
    pub evaluated_candidate_count: usize,
    pub normalization_policy: String,
    pub selection_policy: String,
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
    CalendarResolutionFailed { error_message: String },
    ClockFittingFailed { error_message: String },
    EventRateMaterializationFailed { error_message: String },
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

/// Execute the deterministic business-time clock stage and emit handoff-ready output.
pub fn run_seasonality_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: SeasonalityStageExecutionInput,
) -> SeasonalityStageExecutionReport {
    run_seasonality_calibration_stage_inner(run_spec, calendar_registry, stage_input, None)
}

pub(crate) fn run_seasonality_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: SeasonalityStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(SeasonalityStageProgressUpdate)>,
) -> SeasonalityStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_seasonality_calibration_stage_inner(
            run_spec,
            calendar_registry,
            stage_input,
            None,
        );
    };

    let (progress_sender, progress_receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        let stage_thread = scope.spawn(move || {
            run_seasonality_calibration_stage_inner(
                run_spec,
                calendar_registry,
                stage_input,
                Some(progress_sender),
            )
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match stage_thread.join() {
            Ok(report) => report,
            Err(payload) => panic::resume_unwind(payload),
        }
    })
}

fn run_seasonality_calibration_stage_inner(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: SeasonalityStageExecutionInput,
    progress_sender: Option<Sender<SeasonalityStageProgressUpdate>>,
) -> SeasonalityStageExecutionReport {
    let progress_sender = progress_sender.as_ref();

    let preflight = match validate_preflight(run_spec, &stage_input, progress_sender) {
        Ok(preflight) => preflight,
        Err(report) => return *report,
    };

    let preprocessed = match preprocess_timestamp_events(
        run_spec,
        calendar_registry,
        &stage_input,
        progress_sender,
    ) {
        Ok(preprocessed) => preprocessed,
        Err(report) => return *report,
    };

    let clock_fit = match fit_business_time_clock(
        run_spec,
        calendar_registry,
        &stage_input,
        &preprocessed,
        preflight.stage_solver_settings.clone(),
        progress_sender,
    ) {
        Ok(clock_fit) => clock_fit,
        Err(report) => return *report,
    };

    let payload = match build_normalized_payload(
        run_spec,
        calendar_registry,
        &preflight,
        &preprocessed,
        &clock_fit,
        progress_sender,
    ) {
        Ok(payload) => payload,
        Err(report) => return *report,
    };

    build_success_report(
        run_spec,
        preflight,
        preprocessed,
        clock_fit,
        payload,
        progress_sender,
    )
}

struct SeasonalityPreflight {
    resolved_dataset_id: String,
    resolved_estimator_version: String,
    input_record_count: usize,
    stage_solver_settings: CalibrationOptimizerSettings,
    resolved_stage_seed: CalibrationResolvedStageSeed,
}

fn validate_preflight(
    run_spec: &CalibrationRunSpec,
    stage_input: &SeasonalityStageExecutionInput,
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
) -> Result<SeasonalityPreflight, Box<SeasonalityStageExecutionReport>> {
    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::InputPreprocessing,
        0,
        4,
        Some("run spec"),
    );
    if let Err(source) = run_spec.validate() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        )));
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Seasonality)
    {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::StageIsNotSelectedInRunSpec,
        )));
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Seasonality)
    else {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingStageSolverSettings,
        )));
    };

    if stage_input.timestamp_event_records.is_empty() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingTimestampEventRecords,
        )));
    }

    let resolved_dataset_id = stage_input
        .dataset_id_override
        .clone()
        .unwrap_or_else(|| run_spec.data_inputs.trade_events_data_reference.clone());
    if resolved_dataset_id.trim().is_empty() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyDatasetId,
        )));
    }

    let resolved_estimator_version = stage_input
        .estimator_version_override
        .clone()
        .unwrap_or_else(|| CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string());
    if resolved_estimator_version.trim().is_empty() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyEstimatorVersion,
        )));
    }

    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::InputPreprocessing,
        1,
        4,
        Some("solver settings"),
    );

    Ok(SeasonalityPreflight {
        resolved_dataset_id,
        resolved_estimator_version,
        input_record_count: stage_input.timestamp_event_records.len(),
        stage_solver_settings,
        resolved_stage_seed: resolve_calibration_stage_seed_metadata(
            run_spec,
            CalibrationStage::Seasonality,
        ),
    })
}

fn preprocess_timestamp_events(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &SeasonalityStageExecutionInput,
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
) -> Result<TimestampEventPreprocessingOutput, Box<SeasonalityStageExecutionReport>> {
    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::InputPreprocessing,
        2,
        4,
        Some("timestamp events"),
    );

    let preprocessed = calendar_registry
        .preprocess_timestamp_events(
            &run_spec.data_inputs.exchange_code,
            stage_input.timestamp_event_records.clone(),
            stage_input.settings.session_filter,
            stage_input.settings.sparse_hour_event_threshold,
        )
        .map_err(|source| {
            Box::new(build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::TimestampPreprocessingFailed {
                    error_message: source.to_string(),
                },
            ))
        })?;

    if preprocessed.timestamp_utc_nanos.is_empty() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::NoAcceptedTimestampEventsAfterPreprocessing,
        )));
    }

    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::InputPreprocessing,
        4,
        4,
        Some("timestamp events"),
    );

    Ok(preprocessed)
}

fn fit_business_time_clock(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &SeasonalityStageExecutionInput,
    preprocessed: &TimestampEventPreprocessingOutput,
    stage_solver_settings: CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
) -> Result<FourierBusinessTimeClockFitSummary, Box<SeasonalityStageExecutionReport>> {
    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::ClockFitPreparation,
        0,
        1,
        Some("calendar"),
    );

    let calendar_definition = calendar_registry
        .get_required(&run_spec.data_inputs.exchange_code)
        .map_err(|source| {
            Box::new(build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::CalendarResolutionFailed {
                    error_message: source.to_string(),
                },
            ))
        })?;

    let timezone: Tz = calendar_definition.timezone_name().parse().map_err(|_| {
        Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::CalendarResolutionFailed {
                error_message: format!(
                    "exchange `{}` has invalid timezone name `{}`",
                    run_spec.data_inputs.exchange_code,
                    calendar_definition.timezone_name()
                ),
            },
        ))
    })?;

    let fit_settings =
        resolve_clock_fit_settings(&stage_input.settings, preprocessed, stage_solver_settings);

    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::ClockFitPreparation,
        1,
        1,
        Some("candidate grid"),
    );

    let mut fit_progress_observer = |progress: FourierBusinessTimeClockFitProgress| {
        forward_clock_fit_progress(progress_sender, progress);
    };

    fit_fourier_business_time_clock_model(
        FourierBusinessTimeClockFitInput {
            calendar_definition,
            timezone,
            timestamp_utc_nanos: preprocessed.timestamp_utc_nanos.as_slice(),
            settings: fit_settings,
            cancellation_token: CancellationToken::uncancelled(),
        },
        Some(&mut fit_progress_observer),
    )
    .map_err(|source| {
        Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::ClockFittingFailed {
                error_message: source.to_string(),
            },
        ))
    })
}

fn build_normalized_payload(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    preflight: &SeasonalityPreflight,
    preprocessed: &TimestampEventPreprocessingOutput,
    clock_fit: &FourierBusinessTimeClockFitSummary,
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
) -> Result<SeasonalityStageNormalizedPayload, Box<SeasonalityStageExecutionReport>> {
    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::EventRateMaterialization,
        0,
        preprocessed.timestamp_utc_nanos.len(),
        Some("event rates"),
    );

    let timezone = parse_preprocessed_timezone(run_spec, preprocessed)?;

    let calendar_definition = calendar_registry
        .get_required(&preprocessed.exchange_code)
        .map_err(|source| {
            Box::new(build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::CalendarResolutionFailed {
                    error_message: source.to_string(),
                },
            ))
        })?;

    let business_time_rate_by_event = materialize_event_rates(
        run_spec,
        preprocessed.timestamp_utc_nanos.as_slice(),
        &clock_fit.model,
        timezone,
    )?;

    let open_event_business_time_delta_exposure_hours_by_event =
        materialize_open_event_gap_exposures(
            run_spec,
            preprocessed,
            calendar_definition,
            &clock_fit.model,
            timezone,
        )?;

    let exchange_local_minute_of_week = exchange_local_minute_of_week(run_spec, preprocessed)?;

    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::EventRateMaterialization,
        preprocessed.timestamp_utc_nanos.len(),
        preprocessed.timestamp_utc_nanos.len(),
        Some("event rates"),
    );

    Ok(SeasonalityStageNormalizedPayload {
        schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
        dataset_id: preflight.resolved_dataset_id.clone(),
        exchange_code: preprocessed.exchange_code.clone(),
        trade_events_data_reference: run_spec.data_inputs.trade_events_data_reference.clone(),
        timestamp_event_input_schema_version: preprocessed.input_schema_version.to_string(),
        timestamp_event_preprocessed_schema_version: preprocessed.schema_version.to_string(),
        clock_model_schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
        source_record_positions: preprocessed.source_record_positions.clone(),
        timestamp_utc_nanos: preprocessed.timestamp_utc_nanos.clone(),
        exchange_local_minute_of_week,
        market_state_codes: preprocessed.market_state_codes.clone(),
        business_time_rate_by_event,
        open_event_business_time_delta_exposure_hours_by_event,
        clock_model: clock_fit.model.clone(),
    })
}

fn build_success_report(
    run_spec: &CalibrationRunSpec,
    preflight: SeasonalityPreflight,
    preprocessed: TimestampEventPreprocessingOutput,
    clock_fit: FourierBusinessTimeClockFitSummary,
    payload: SeasonalityStageNormalizedPayload,
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
) -> SeasonalityStageExecutionReport {
    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::ReportAssembly,
        0,
        1,
        Some("report"),
    );

    let handoffs = build_seasonality_stage_handoff_outputs(run_spec);
    let clock_model = &clock_fit.model;

    let report = SeasonalityStageExecutionReport {
        schema_version: CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Seasonality,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
        handoffs,
        normalized_stage_payload: Some(payload),
        metadata: Some(SeasonalityStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: preflight.resolved_stage_seed.run_global_seed,
            stage_seed: preflight.resolved_stage_seed.stage_seed,
            stage_seed_source: preflight.resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: preflight.stage_solver_settings,
            dataset_id: preflight.resolved_dataset_id,
            estimator_version: preflight.resolved_estimator_version,
            input_record_count: preflight.input_record_count,
            accepted_record_count: preprocessed.diagnostics.accepted_record_count,
            filtered_record_count: preprocessed.diagnostics.filtered_record_count,
            selected_harmonic_orders: clock_model.harmonic_orders(),
            evaluated_candidate_count: clock_fit.evaluated_candidate_count,
            normalization_policy: clock_model.normalization_policy.clone(),
            selection_policy: clock_model.selection_policy.clone(),
        }),
        failure: None,
    };

    emit_seasonality_progress(
        progress_sender,
        SeasonalityStageProgressPhase::ReportAssembly,
        1,
        1,
        Some("report"),
    );

    report
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
