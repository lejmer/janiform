use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::super::objective::CalibrationObjectiveTermInput;
use super::super::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunMetadata,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use super::seasonality::{CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload};
use crate::adapters::precheck_structured_hawkes_kernel_parameter_domain;
use crate::calendar::ExchangeCalendarRegistry;
use janiform_calibration_artifacts::{
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource, HawkesStageFittedParameters,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_foundations::numeric::{arithmetic_mean, coefficient_of_variation};
use janiform_model_kernels::hawkes::{
    HawkesKernelParameterValidationReport, HawkesKernelParameters,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};
/// Candidate parameterization and objective inputs for Hawkes stage fitting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HawkesStageObservedSourceRegime {
    pub observation_duration_hours: f64,
    pub open_session_duration_hours: f64,
    pub total_tick_count: usize,
    pub open_session_tick_count: usize,
    pub closed_session_tick_count: usize,
    pub open_session_buy_tick_count: usize,
    pub open_session_sell_tick_count: usize,
    pub open_session_mean_interarrival_seconds: f64,
    pub open_session_interarrival_coefficient_of_variation: f64,
    pub open_session_short_gap_threshold_seconds: f64,
    pub open_session_short_gap_ratio: f64,
    pub known_news_open_event_count: usize,
    pub open_session_known_news_activity_share: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageObservedSourceRegimeError {
    InvalidObservationDurationHours {
        value: f64,
    },
    InvalidOpenSessionDurationHours {
        value: f64,
    },
    MissingTotalTickCount,
    MissingOpenSessionTickCount,
    InconsistentOpenSessionTickCount {
        buy_tick_count: usize,
        sell_tick_count: usize,
        open_session_tick_count: usize,
    },
    InconsistentTotalTickCount {
        open_session_tick_count: usize,
        closed_session_tick_count: usize,
        total_tick_count: usize,
    },
    InvalidMeanInterarrivalSeconds {
        value: f64,
    },
    InvalidInterarrivalCoefficientOfVariation {
        value: f64,
    },
    InvalidShortGapThresholdSeconds {
        value: f64,
    },
    InvalidShortGapRatio {
        value: f64,
    },
    InvalidKnownNewsActivityShare {
        value: f64,
    },
}

impl Display for HawkesStageObservedSourceRegimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidObservationDurationHours { value } => write!(
                formatter,
                "source_regime observation_duration_hours must be finite and positive, found {value}"
            ),
            Self::InvalidOpenSessionDurationHours { value } => write!(
                formatter,
                "source_regime open_session_duration_hours must be finite and positive, found {value}"
            ),
            Self::MissingTotalTickCount => {
                write!(
                    formatter,
                    "source_regime total_tick_count must be strictly positive"
                )
            }
            Self::MissingOpenSessionTickCount => write!(
                formatter,
                "source_regime open_session_tick_count must be strictly positive"
            ),
            Self::InconsistentOpenSessionTickCount {
                buy_tick_count,
                sell_tick_count,
                open_session_tick_count,
            } => write!(
                formatter,
                "source_regime open buy and sell counts must sum to open_session_tick_count, found buy={buy_tick_count}, sell={sell_tick_count}, open_session_tick_count={open_session_tick_count}"
            ),
            Self::InconsistentTotalTickCount {
                open_session_tick_count,
                closed_session_tick_count,
                total_tick_count,
            } => write!(
                formatter,
                "source_regime open and closed tick counts must sum to total_tick_count, found open={open_session_tick_count}, closed={closed_session_tick_count}, total_tick_count={total_tick_count}"
            ),
            Self::InvalidMeanInterarrivalSeconds { value } => write!(
                formatter,
                "source_regime open_session_mean_interarrival_seconds must be finite and positive, found {value}"
            ),
            Self::InvalidInterarrivalCoefficientOfVariation { value } => write!(
                formatter,
                "source_regime open_session_interarrival_coefficient_of_variation must be finite and non-negative, found {value}"
            ),
            Self::InvalidShortGapThresholdSeconds { value } => write!(
                formatter,
                "source_regime open_session_short_gap_threshold_seconds must be finite and positive, found {value}"
            ),
            Self::InvalidShortGapRatio { value } => write!(
                formatter,
                "source_regime open_session_short_gap_ratio must be finite and between 0 and 1, found {value}"
            ),
            Self::InvalidKnownNewsActivityShare { value } => write!(
                formatter,
                "source_regime open_session_known_news_activity_share must be finite and between 0 and 1, found {value}"
            ),
        }
    }
}

impl Error for HawkesStageObservedSourceRegimeError {}

impl HawkesStageObservedSourceRegime {
    fn validate(&self) -> Result<(), HawkesStageObservedSourceRegimeError> {
        if !self.observation_duration_hours.is_finite() || self.observation_duration_hours <= 0.0 {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidObservationDurationHours {
                    value: self.observation_duration_hours,
                },
            );
        }
        if !self.open_session_duration_hours.is_finite() || self.open_session_duration_hours <= 0.0
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidOpenSessionDurationHours {
                    value: self.open_session_duration_hours,
                },
            );
        }
        if self.total_tick_count == 0 {
            return Err(HawkesStageObservedSourceRegimeError::MissingTotalTickCount);
        }
        if self.open_session_tick_count == 0 {
            return Err(HawkesStageObservedSourceRegimeError::MissingOpenSessionTickCount);
        }
        if self.open_session_buy_tick_count + self.open_session_sell_tick_count
            != self.open_session_tick_count
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InconsistentOpenSessionTickCount {
                    buy_tick_count: self.open_session_buy_tick_count,
                    sell_tick_count: self.open_session_sell_tick_count,
                    open_session_tick_count: self.open_session_tick_count,
                },
            );
        }
        if self.open_session_tick_count + self.closed_session_tick_count != self.total_tick_count {
            return Err(
                HawkesStageObservedSourceRegimeError::InconsistentTotalTickCount {
                    open_session_tick_count: self.open_session_tick_count,
                    closed_session_tick_count: self.closed_session_tick_count,
                    total_tick_count: self.total_tick_count,
                },
            );
        }
        if !self.open_session_mean_interarrival_seconds.is_finite()
            || self.open_session_mean_interarrival_seconds <= 0.0
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidMeanInterarrivalSeconds {
                    value: self.open_session_mean_interarrival_seconds,
                },
            );
        }
        if !self
            .open_session_interarrival_coefficient_of_variation
            .is_finite()
            || self.open_session_interarrival_coefficient_of_variation < 0.0
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidInterarrivalCoefficientOfVariation {
                    value: self.open_session_interarrival_coefficient_of_variation,
                },
            );
        }
        if !self.open_session_short_gap_threshold_seconds.is_finite()
            || self.open_session_short_gap_threshold_seconds <= 0.0
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidShortGapThresholdSeconds {
                    value: self.open_session_short_gap_threshold_seconds,
                },
            );
        }
        if !self.open_session_short_gap_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.open_session_short_gap_ratio)
        {
            return Err(HawkesStageObservedSourceRegimeError::InvalidShortGapRatio {
                value: self.open_session_short_gap_ratio,
            });
        }
        if !self.open_session_known_news_activity_share.is_finite()
            || !(0.0..=1.0).contains(&self.open_session_known_news_activity_share)
        {
            return Err(
                HawkesStageObservedSourceRegimeError::InvalidKnownNewsActivityShare {
                    value: self.open_session_known_news_activity_share,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HawkesStageDirectionalLabelDiagnostics {
    pub policy_id: String,
    pub first_inferred_trade_sign: String,
    pub provided_label_count: usize,
    pub inferred_label_count: usize,
    pub flat_move_carry_count: usize,
    pub price_up_inference_count: usize,
    pub price_down_inference_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageDirectionalLabelDiagnosticsError {
    MissingPolicyId,
    MissingFirstInferredTradeSign,
    InconsistentInferenceBreakdown {
        inferred_label_count: usize,
        component_count: usize,
    },
}

impl Display for HawkesStageDirectionalLabelDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPolicyId => {
                write!(
                    formatter,
                    "directional_label_diagnostics policy_id must be non-empty"
                )
            }
            Self::MissingFirstInferredTradeSign => write!(
                formatter,
                "directional_label_diagnostics first_inferred_trade_sign must be non-empty"
            ),
            Self::InconsistentInferenceBreakdown {
                inferred_label_count,
                component_count,
            } => write!(
                formatter,
                "directional_label_diagnostics inference components must not exceed inferred_label_count, found inferred_label_count={inferred_label_count} and component_count={component_count}"
            ),
        }
    }
}

impl Error for HawkesStageDirectionalLabelDiagnosticsError {}

impl HawkesStageDirectionalLabelDiagnostics {
    fn validate(&self) -> Result<(), HawkesStageDirectionalLabelDiagnosticsError> {
        if self.policy_id.trim().is_empty() {
            return Err(HawkesStageDirectionalLabelDiagnosticsError::MissingPolicyId);
        }
        if self.first_inferred_trade_sign.trim().is_empty() {
            return Err(HawkesStageDirectionalLabelDiagnosticsError::MissingFirstInferredTradeSign);
        }

        let component_count = self.flat_move_carry_count
            + self.price_up_inference_count
            + self.price_down_inference_count;
        if component_count > self.inferred_label_count {
            return Err(
                HawkesStageDirectionalLabelDiagnosticsError::InconsistentInferenceBreakdown {
                    inferred_label_count: self.inferred_label_count,
                    component_count,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageCandidateInput {
    pub candidate_id: String,
    pub lambda_bar: f64,
    pub beta: f64,
    pub structured_alpha: HawkesStructuredAlphaParameterization,
    pub structured_rho: HawkesStructuredRhoParameterization,
    pub self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    pub stability_numeric_tolerance: f64,
    pub source_regime: HawkesStageObservedSourceRegime,
    pub directional_label_diagnostics: HawkesStageDirectionalLabelDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageCandidateInputError {
    MissingCandidateId,
    InvalidLambdaBar {
        value: f64,
    },
    InvalidBeta {
        value: f64,
    },
    InvalidStabilityNumericTolerance {
        value: f64,
    },
    InvalidSourceRegime {
        source: HawkesStageObservedSourceRegimeError,
    },
    InvalidDirectionalLabelDiagnostics {
        source: HawkesStageDirectionalLabelDiagnosticsError,
    },
}

impl Display for HawkesStageCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => write!(formatter, "candidate_id must be non-empty"),
            Self::InvalidLambdaBar { value } => write!(
                formatter,
                "candidate lambda_bar must be finite for objective evaluation, found {value}"
            ),
            Self::InvalidBeta { value } => write!(
                formatter,
                "candidate beta must be finite for objective evaluation, found {value}"
            ),
            Self::InvalidStabilityNumericTolerance { value } => write!(
                formatter,
                "candidate stability_numeric_tolerance must be finite, found {value}"
            ),
            Self::InvalidSourceRegime { source } => {
                write!(formatter, "candidate source_regime is invalid: {source}")
            }
            Self::InvalidDirectionalLabelDiagnostics { source } => write!(
                formatter,
                "candidate directional_label_diagnostics are invalid: {source}"
            ),
        }
    }
}

impl Error for HawkesStageCandidateInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSourceRegime { source } => Some(source),
            Self::InvalidDirectionalLabelDiagnostics { source } => Some(source),
            _ => None,
        }
    }
}

impl HawkesStageCandidateInput {
    fn validate(&self) -> Result<(), HawkesStageCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(HawkesStageCandidateInputError::MissingCandidateId);
        }
        if !self.lambda_bar.is_finite() {
            return Err(HawkesStageCandidateInputError::InvalidLambdaBar {
                value: self.lambda_bar,
            });
        }
        if !self.beta.is_finite() {
            return Err(HawkesStageCandidateInputError::InvalidBeta { value: self.beta });
        }
        if !self.stability_numeric_tolerance.is_finite() {
            return Err(
                HawkesStageCandidateInputError::InvalidStabilityNumericTolerance {
                    value: self.stability_numeric_tolerance,
                },
            );
        }
        self.source_regime
            .validate()
            .map_err(|source| HawkesStageCandidateInputError::InvalidSourceRegime { source })?;
        self.directional_label_diagnostics
            .validate()
            .map_err(|source| {
                HawkesStageCandidateInputError::InvalidDirectionalLabelDiagnostics { source }
            })?;

        Ok(())
    }
}

/// Upstream seasonality handoff input resolved for Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageSeasonalityInput {
    InMemoryOutput {
        payload: SeasonalityStageNormalizedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: SeasonalityStageNormalizedPayload,
    },
}

impl HawkesStageSeasonalityInput {
    fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference),
        }
    }

    fn dataset_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.dataset_id.as_str()),
            Self::PersistedArtifactReference { .. } => None,
        }
    }

    fn payload(&self) -> &SeasonalityStageNormalizedPayload {
        match self {
            Self::InMemoryOutput { payload } => payload,
            Self::PersistedArtifactReference { payload, .. } => payload,
        }
    }
}

/// Input payload for standalone or pipeline-integrated Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageExecutionInput {
    pub seasonality_input: HawkesStageSeasonalityInput,
    pub candidate_inputs: Vec<HawkesStageCandidateInput>,
}

impl HawkesStageExecutionInput {
    pub fn from_in_memory_seasonality_output(
        seasonality_payload: SeasonalityStageNormalizedPayload,
        candidate_inputs: Vec<HawkesStageCandidateInput>,
    ) -> Self {
        Self {
            seasonality_input: HawkesStageSeasonalityInput::InMemoryOutput {
                payload: seasonality_payload,
            },
            candidate_inputs,
        }
    }

    pub fn from_persisted_seasonality_artifact_reference(
        artifact_reference: impl Into<String>,
        seasonality_payload: SeasonalityStageNormalizedPayload,
        candidate_inputs: Vec<HawkesStageCandidateInput>,
    ) -> Self {
        Self {
            seasonality_input: HawkesStageSeasonalityInput::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
                payload: seasonality_payload,
            },
            candidate_inputs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HawkesObservedActivityTargets {
    deseasonalized_open_event_rate_per_hour: f64,
    open_factor_exposure_hours: f64,
    deseasonalized_mean_interarrival_seconds: f64,
    deseasonalized_interarrival_coefficient_of_variation: f64,
    deseasonalized_short_gap_threshold_seconds: f64,
    deseasonalized_short_gap_ratio: f64,
    deseasonalized_known_news_rate_per_hour: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeasonalityScaledStationarityCheck {
    max_seasonality_factor: f64,
    spectral_radius_alpha_over_beta: f64,
    effective_branching_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageObjectiveTermDiagnostic {
    pub term_id: String,
    pub weight: f64,
    pub observed_value: f64,
    pub model_implied_value: f64,
    pub scale: f64,
    pub absolute_error: f64,
    pub normalized_error: f64,
    pub weighted_contribution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageImpliedMetricsDiagnostic {
    pub baseline_buy_intensity_per_hour: f64,
    pub baseline_sell_intensity_per_hour: f64,
    pub observed_open_event_rate_per_hour: f64,
    pub deseasonalized_open_event_rate_per_hour_target: f64,
    pub lambda_bar_scaling_ratio_applied: f64,
    pub max_runtime_hourly_seasonality_factor: f64,
    pub max_runtime_effective_branching_ratio: f64,
    pub runtime_effective_branching_margin: f64,
    pub model_implied_open_event_rate_per_hour: f64,
    pub model_implied_open_buy_share: f64,
    pub model_implied_open_sell_share: f64,
    pub model_implied_interarrival_coefficient_of_variation: f64,
    pub model_implied_short_gap_ratio: f64,
    pub model_implied_known_news_activity_share: f64,
    pub model_implied_expected_event_count_over_observation_window: f64,
    pub endogenous_branching_ratio: f64,
    pub self_excitation_ratio: f64,
    pub cross_excitation_ratio: f64,
    pub exogenous_self_loading_ratio: f64,
    pub exogenous_cross_loading_ratio: f64,
    pub stability_margin: f64,
    pub event_volume_ratio_vs_total_observed: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageCandidateDiagnostic {
    pub candidate_id: String,
    pub raw_objective_value: f64,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub constraint_violation_count: usize,
    pub objective_terms: Vec<HawkesStageObjectiveTermDiagnostic>,
    pub source_regime: HawkesStageObservedSourceRegime,
    pub directional_label_diagnostics: HawkesStageDirectionalLabelDiagnostics,
    pub implied_metrics: HawkesStageImpliedMetricsDiagnostic,
    pub warnings: Vec<String>,
    pub validation_report: HawkesKernelParameterValidationReport,
}

/// Stable Hawkes-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: HawkesStageFittedParameters,
    pub candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    pub seasonality_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasonality_dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasonality_artifact_reference: Option<String>,
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

/// Structured failure modes for Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum HawkesStageExecutionFailure {
    InvalidRunSpec {
        error_message: String,
    },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    SeasonalityInputTransportMismatch {
        expected_transport: CalibrationStageHandoffTransport,
        provided_transport: CalibrationStageHandoffTransport,
    },
    MissingArtifactReferenceInSeasonalityInput,
    InvalidArtifactReferenceInSeasonalityInput {
        artifact_reference: String,
    },
    SeasonalityArtifactReferenceMismatch {
        expected_artifact_reference: String,
        provided_artifact_reference: String,
    },
    MissingHawkesCandidateInputs,
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        error_message: String,
    },
    ConstraintViolationsForAllCandidates {
        candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    },
    CandidateConstructionFailed {
        candidate_id: String,
        error_message: String,
    },
}

/// Structured Hawkes-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<HawkesStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<HawkesStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HawkesStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<HawkesStageExecutionFailure>,
}

/// Execute the Hawkes stage with structured constraint checks for each candidate.
pub fn run_hawkes_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: HawkesStageExecutionInput,
) -> HawkesStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Hawkes)
    {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Hawkes)
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if stage_input.candidate_inputs.is_empty() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingHawkesCandidateInputs,
        );
    }

    let expected_seasonality_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Seasonality
            && stage_handoff.to_stage == CalibrationStage::Hawkes
    });
    let expected_transport = expected_seasonality_handoff
        .map(|stage_handoff| stage_handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.seasonality_input.transport();

    if provided_transport != expected_transport {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::SeasonalityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::MissingArtifactReferenceInSeasonalityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidArtifactReferenceInSeasonalityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_seasonality_handoff
            && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
            && expected_artifact_reference != provided_artifact_reference
        {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::SeasonalityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.to_string(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::Hawkes);
    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        resolved_stage_seed.stage_seed,
        stage_solver_settings.clone(),
        hawkes_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut seen_candidate_ids = BTreeSet::new();
    let mut validation_reports = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut objective_term_diagnostics = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut implied_metrics_diagnostics = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut warning_diagnostics = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut adapter_candidate_inputs = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut adjusted_candidate_inputs = Vec::with_capacity(stage_input.candidate_inputs.len());
    let seasonality_payload = stage_input.seasonality_input.payload();
    let max_runtime_hourly_seasonality_factor =
        match compute_runtime_hourly_seasonality_factor(calendar_registry, seasonality_payload) {
            Ok(value) => value,
            Err(error_message) => {
                return build_hawkes_stage_failure_report(
                    run_spec,
                    HawkesStageExecutionFailure::CandidateConstructionFailed {
                        candidate_id: "runtime_hourly_seasonality_envelope".to_string(),
                        error_message,
                    },
                );
            }
        };

    for candidate_input in &stage_input.candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }

        let observed_activity_targets = derive_observed_activity_targets(
            candidate_input.source_regime.clone(),
            seasonality_payload,
        );
        let (adjusted_candidate_input, lambda_bar_scaling_ratio_applied) =
            adjust_candidate_for_deseasonalized_activity(
                candidate_input,
                observed_activity_targets,
            );
        let validation_report = precheck_structured_hawkes_kernel_parameter_domain(
            adjusted_candidate_input.lambda_bar,
            adjusted_candidate_input.beta,
            adjusted_candidate_input.structured_alpha,
            adjusted_candidate_input.structured_rho,
            adjusted_candidate_input.self_dominance_policy,
            adjusted_candidate_input.stability_numeric_tolerance,
        );

        let mut external_constraint_violation_count = validation_report.summary.failed_check_count;
        let (objective_terms, term_diagnostics, implied_metrics, mut warnings) =
            build_candidate_objective_terms(
                &adjusted_candidate_input,
                observed_activity_targets,
                lambda_bar_scaling_ratio_applied,
                max_runtime_hourly_seasonality_factor,
                validation_report.derived_alpha,
                validation_report.derived_rho,
            );

        if let Some(stationarity_check) =
            validation_report.derived_alpha.and_then(|derived_alpha| {
                check_seasonality_scaled_stationarity(
                    max_runtime_hourly_seasonality_factor,
                    derived_alpha,
                    adjusted_candidate_input.beta,
                )
            })
        {
            external_constraint_violation_count =
                external_constraint_violation_count.saturating_add(1);
            warnings.push(format!(
                "seasonality-scaled effective branching ratio {:.6} reaches or exceeds 1.0 with max seasonality factor {:.6}",
                stationarity_check.effective_branching_ratio,
                stationarity_check.max_seasonality_factor,
            ));
        }

        adjusted_candidate_inputs.push(adjusted_candidate_input.clone());
        validation_reports.push(validation_report);
        objective_term_diagnostics.push(term_diagnostics);
        implied_metrics_diagnostics.push(implied_metrics);
        warning_diagnostics.push(warnings);
        adapter_candidate_inputs.push(CalibrationOptimizerAdapterCandidateInput {
            candidate_id: adjusted_candidate_input.candidate_id.clone(),
            objective_terms,
            parameter_values: vec![
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "lambda_bar".to_string(),
                    value: adjusted_candidate_input.lambda_bar,
                },
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "beta".to_string(),
                    value: adjusted_candidate_input.beta,
                },
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "stability_numeric_tolerance".to_string(),
                    value: adjusted_candidate_input.stability_numeric_tolerance,
                },
            ],
            external_constraint_violation_count,
        });
    }

    let optimizer_adapter_output = match run_constrained_optimizer_adapter(
        &optimizer_adapter_configuration,
        &adapter_candidate_inputs,
    ) {
        Ok(output) => output,

        Err(CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId { candidate_id },
            );
        }

        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic> =
        Vec::with_capacity(stage_input.candidate_inputs.len());
    for (
        (
            (((candidate_input, validation_report), optimizer_diagnostic), objective_terms),
            implied_metrics,
        ),
        warnings,
    ) in adjusted_candidate_inputs
        .iter()
        .zip(validation_reports.into_iter())
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
        .zip(objective_term_diagnostics.into_iter())
        .zip(implied_metrics_diagnostics.into_iter())
        .zip(warning_diagnostics.into_iter())
    {
        candidate_diagnostics.push(HawkesStageCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: optimizer_diagnostic.raw_objective_value,
            constraint_penalty: optimizer_diagnostic.constraint_penalty,
            penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
            constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
            objective_terms,
            source_regime: candidate_input.source_regime.clone(),
            directional_label_diagnostics: candidate_input.directional_label_diagnostics.clone(),
            implied_metrics,
            warnings,
            validation_report,
        });
    }

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = stage_input
        .candidate_inputs
        .iter()
        .position(|candidate| candidate.candidate_id == *selected_candidate_id)
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message:
                    "selected candidate id was not found in stage_input.candidate_inputs"
                        .to_string(),
            },
        );
    };

    let selected_candidate_input = &adjusted_candidate_inputs[selected_candidate_position];
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];

    let Some(derived_alpha) = selected_candidate_diagnostic
        .validation_report
        .derived_alpha
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived alpha"
                        .to_string(),
            },
        );
    };

    let Some(derived_rho) = selected_candidate_diagnostic.validation_report.derived_rho else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived rho"
                        .to_string(),
            },
        );
    };

    let kernel_parameters = match HawkesKernelParameters::try_new_with_stability_tolerance(
        selected_candidate_input.lambda_bar,
        selected_candidate_input.beta,
        derived_alpha,
        derived_rho,
        selected_candidate_input.stability_numeric_tolerance,
    ) {
        Ok(kernel_parameters) => kernel_parameters,

        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let valid_candidate_count = optimizer_adapter_output
        .candidate_diagnostics
        .iter()
        .filter(|candidate_diagnostic| candidate_diagnostic.total_constraint_violation_count == 0)
        .count();

    let selected_candidate_raw_objective = selected_candidate_diagnostic.raw_objective_value;
    let selected_candidate_penalized_objective =
        selected_candidate_diagnostic.penalized_objective_value;

    let fitted_stage_payload = HawkesStageFittedPayload {
        schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha: selected_candidate_input.structured_alpha,
            structured_rho: selected_candidate_input.structured_rho,
            self_dominance_policy: selected_candidate_input.self_dominance_policy,
            stability_numeric_tolerance: selected_candidate_input.stability_numeric_tolerance,
        },
        candidate_diagnostics,
        seasonality_input_transport: stage_input.seasonality_input.transport(),
        seasonality_dataset_id: stage_input
            .seasonality_input
            .dataset_id()
            .map(ToOwned::to_owned),
        seasonality_artifact_reference: stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: build_hawkes_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(HawkesStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: stage_input.candidate_inputs.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
        }),
        failure: None,
    }
}

fn build_candidate_objective_terms(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_hourly_seasonality_factor: f64,
    derived_alpha: Option<janiform_model_kernels::hawkes::HawkesKernelLoadings>,
    derived_rho: Option<janiform_model_kernels::hawkes::HawkesKernelLoadings>,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    let Some(derived_alpha) = derived_alpha else {
        return invalid_candidate_objective_bundle(
            candidate_input,
            "candidate did not derive alpha",
        );
    };
    let Some(derived_rho) = derived_rho else {
        return invalid_candidate_objective_bundle(candidate_input, "candidate did not derive rho");
    };

    let implied_metrics = derive_candidate_implied_metrics(
        candidate_input,
        observed_activity_targets,
        lambda_bar_scaling_ratio_applied,
        max_runtime_hourly_seasonality_factor,
        derived_alpha,
        derived_rho,
    );
    let mut warnings = Vec::new();

    if implied_metrics.event_volume_ratio_vs_total_observed > 1.5 {
        warnings.push(format!(
            "model implied event count over the observation window is {:.3}x the observed total tick count",
            implied_metrics.event_volume_ratio_vs_total_observed
        ));
    }
    if implied_metrics.model_implied_open_event_rate_per_hour
        > implied_metrics.observed_open_event_rate_per_hour * 1.5
    {
        warnings.push(format!(
            "model implied open-session rate {:.3} exceeds the observed open-session rate {:.3} by more than 50%",
            implied_metrics.model_implied_open_event_rate_per_hour,
            implied_metrics.observed_open_event_rate_per_hour
        ));
    }

    let mut objective_terms = Vec::new();
    let mut term_diagnostics = Vec::new();

    push_objective_term(
        "open_session_event_rate",
        3.0,
        implied_metrics.observed_open_event_rate_per_hour,
        implied_metrics.model_implied_open_event_rate_per_hour,
        implied_metrics.observed_open_event_rate_per_hour.max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "directional_imbalance_magnitude",
        1.5,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime),
        (implied_metrics.model_implied_open_sell_share - 0.5).abs() * 2.0,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime).max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "interarrival_coefficient_of_variation",
        1.25,
        observed_activity_targets.deseasonalized_interarrival_coefficient_of_variation,
        implied_metrics.model_implied_interarrival_coefficient_of_variation,
        observed_activity_targets
            .deseasonalized_interarrival_coefficient_of_variation
            .max(0.25),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "short_gap_ratio",
        1.0,
        observed_activity_targets.deseasonalized_short_gap_ratio,
        implied_metrics.model_implied_short_gap_ratio,
        observed_activity_targets
            .deseasonalized_short_gap_ratio
            .max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "observation_window_event_count",
        2.0,
        candidate_input.source_regime.open_session_tick_count as f64,
        implied_metrics.model_implied_expected_event_count_over_observation_window,
        (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    if candidate_input.source_regime.known_news_open_event_count > 0 {
        push_objective_term(
            "known_news_activity_share",
            0.75,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share,
            implied_metrics.model_implied_known_news_activity_share,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share
                .max(0.02),
            &mut objective_terms,
            &mut term_diagnostics,
        );
    }

    (objective_terms, term_diagnostics, implied_metrics, warnings)
}

fn invalid_candidate_objective_bundle(
    candidate_input: &HawkesStageCandidateInput,
    warning_message: &str,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    (
        vec![CalibrationObjectiveTermInput {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_score: 1_000_000.0,
        }],
        vec![HawkesStageObjectiveTermDiagnostic {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_value: 0.0,
            model_implied_value: 1_000_000.0,
            scale: 1.0,
            absolute_error: 1_000_000.0,
            normalized_error: 1_000_000.0,
            weighted_contribution: 1_000_000.0,
        }],
        HawkesStageImpliedMetricsDiagnostic {
            baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
            baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
            observed_open_event_rate_per_hour: observed_open_session_event_rate_per_hour(
                &candidate_input.source_regime,
            ),
            deseasonalized_open_event_rate_per_hour_target: 0.0,
            lambda_bar_scaling_ratio_applied: 1.0,
            max_runtime_hourly_seasonality_factor: 1.0,
            max_runtime_effective_branching_ratio: 0.0,
            runtime_effective_branching_margin: 1.0,
            model_implied_open_event_rate_per_hour: 0.0,
            model_implied_open_buy_share: 0.5,
            model_implied_open_sell_share: 0.5,
            model_implied_interarrival_coefficient_of_variation: 0.0,
            model_implied_short_gap_ratio: 0.0,
            model_implied_known_news_activity_share: 0.0,
            model_implied_expected_event_count_over_observation_window: 0.0,
            endogenous_branching_ratio: 0.0,
            self_excitation_ratio: 0.0,
            cross_excitation_ratio: 0.0,
            exogenous_self_loading_ratio: 0.0,
            exogenous_cross_loading_ratio: 0.0,
            stability_margin: 0.0,
            event_volume_ratio_vs_total_observed: 0.0,
        },
        vec![warning_message.to_string()],
    )
}

fn derive_observed_activity_targets(
    source_regime: HawkesStageObservedSourceRegime,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> HawkesObservedActivityTargets {
    const MAX_DESEASONALIZED_GAP_SECONDS: f64 = 3_600.0;
    const OPEN_MARKET_STATE_CODE: u8 = 1;

    let open_event_points: Vec<(i64, f64)> = seasonality_payload
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(
            seasonality_payload
                .normalized_combined_factor_by_event
                .iter()
                .copied(),
        )
        .zip(seasonality_payload.market_state_codes.iter().copied())
        .filter_map(|((timestamp_utc_nanos, factor), market_state_code)| {
            if market_state_code == OPEN_MARKET_STATE_CODE {
                Some((timestamp_utc_nanos, factor.max(1.0e-9)))
            } else {
                None
            }
        })
        .collect();

    let mut transformed_open_gaps_seconds: Vec<f64> =
        Vec::with_capacity(open_event_points.len().saturating_sub(1));
    let mut same_window_gap_seconds_sum = 0.0;
    let mut seasonality_weighted_gap_seconds_sum = 0.0;

    for (
        (previous_timestamp_utc_nanos, previous_factor),
        (current_timestamp_utc_nanos, current_factor),
    ) in open_event_points
        .iter()
        .copied()
        .zip(open_event_points.iter().copied().skip(1))
    {
        let raw_gap_seconds = ((current_timestamp_utc_nanos - previous_timestamp_utc_nanos) as f64
            / 1_000_000_000.0)
            .max(0.0);
        if raw_gap_seconds > MAX_DESEASONALIZED_GAP_SECONDS {
            continue;
        }

        let average_factor = ((previous_factor + current_factor) / 2.0).max(1.0e-9);

        same_window_gap_seconds_sum += raw_gap_seconds;
        seasonality_weighted_gap_seconds_sum += raw_gap_seconds * average_factor;

        transformed_open_gaps_seconds.push(raw_gap_seconds * average_factor);
    }

    let fallback_mean_factor = arithmetic_mean(
        open_event_points
            .iter()
            .map(|(_, factor)| *factor)
            .collect::<Vec<f64>>()
            .as_slice(),
    )
    .unwrap_or(1.0)
    .max(1.0e-9);

    let mean_open_factor = if same_window_gap_seconds_sum > 0.0 {
        (seasonality_weighted_gap_seconds_sum / same_window_gap_seconds_sum).max(1.0e-9)
    } else {
        fallback_mean_factor
    };

    let open_factor_exposure_hours =
        (source_regime.open_session_duration_hours * mean_open_factor).max(1.0e-9);
    let deseasonalized_open_event_rate_per_hour =
        source_regime.open_session_tick_count as f64 / open_factor_exposure_hours;
    let deseasonalized_mean_interarrival_seconds =
        arithmetic_mean(transformed_open_gaps_seconds.as_slice())
            .unwrap_or(source_regime.open_session_mean_interarrival_seconds * mean_open_factor)
            .max(1.0e-6);
    let deseasonalized_interarrival_coefficient_of_variation =
        coefficient_of_variation(transformed_open_gaps_seconds.as_slice());
    let deseasonalized_short_gap_threshold_seconds =
        (deseasonalized_mean_interarrival_seconds * 0.5).clamp(1.0, 300.0);
    let deseasonalized_short_gap_ratio = if transformed_open_gaps_seconds.is_empty() {
        0.0
    } else {
        transformed_open_gaps_seconds
            .iter()
            .filter(|gap_seconds| **gap_seconds <= deseasonalized_short_gap_threshold_seconds)
            .count() as f64
            / transformed_open_gaps_seconds.len() as f64
    };
    let deseasonalized_known_news_rate_per_hour =
        source_regime.known_news_open_event_count as f64 / open_factor_exposure_hours.max(1.0e-9);

    HawkesObservedActivityTargets {
        deseasonalized_open_event_rate_per_hour,
        open_factor_exposure_hours,
        deseasonalized_mean_interarrival_seconds,
        deseasonalized_interarrival_coefficient_of_variation,
        deseasonalized_short_gap_threshold_seconds,
        deseasonalized_short_gap_ratio,
        deseasonalized_known_news_rate_per_hour,
    }
}

fn compute_runtime_hourly_seasonality_factor(
    calendar_registry: &ExchangeCalendarRegistry,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> Result<f64, String> {
    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

    let Some(first_timestamp_utc_nanos) = seasonality_payload.timestamp_utc_nanos.first().copied()
    else {
        return Ok(1.0);
    };
    let Some(last_timestamp_utc_nanos) = seasonality_payload.timestamp_utc_nanos.last().copied()
    else {
        return Ok(1.0);
    };

    let timezone_name = calendar_registry
        .get_required(seasonality_payload.exchange_code.as_str())
        .map_err(|error| error.to_string())?
        .timezone_name()
        .to_string();
    let timezone: Tz = timezone_name.parse().map_err(|_| {
        format!(
            "exchange `{}` has invalid timezone name `{timezone_name}`",
            seasonality_payload.exchange_code
        )
    })?;

    let aligned_range_start_utc_nanos = ceil_to_hour_utc_nanos(first_timestamp_utc_nanos);
    let aligned_range_end_utc_nanos = floor_to_hour_utc_nanos(last_timestamp_utc_nanos);

    if aligned_range_end_utc_nanos <= aligned_range_start_utc_nanos {
        return Ok(seasonality_payload
            .normalized_combined_factor_by_event
            .iter()
            .copied()
            .reduce(f64::max)
            .unwrap_or(1.0)
            .max(1.0));
    }

    let mut maximum_hourly_factor: f64 = 1.0;
    let mut interval_start_utc_nanos = aligned_range_start_utc_nanos;
    while interval_start_utc_nanos < aligned_range_end_utc_nanos {
        maximum_hourly_factor = maximum_hourly_factor.max(structural_seasonality_factor_at_hour(
            seasonality_payload,
            timezone,
            interval_start_utc_nanos,
        )?);
        interval_start_utc_nanos += ONE_HOUR_NANOS;
    }

    Ok(maximum_hourly_factor)
}

fn structural_seasonality_factor_at_hour(
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    timezone: Tz,
    timestamp_utc_nanos: i64,
) -> Result<f64, String> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);
    let exchange_local_timestamp = timestamp_utc.with_timezone(&timezone);
    let minute_of_day =
        exchange_local_timestamp.hour() as usize * 60 + exchange_local_timestamp.minute() as usize;
    let intraday_bin_index =
        minute_of_day / seasonality_payload.intraday_minutes_per_bin.max(1) as usize;
    let weekday_index = exchange_local_timestamp.weekday().number_from_monday() as usize - 1;
    let day_of_year_index = exchange_local_timestamp.ordinal0() as usize;

    let intraday_factor = seasonality_payload
        .intraday_factor_by_bin
        .get(intraday_bin_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "intraday factor bin index {intraday_bin_index} is out of range for payload with {} bins",
                seasonality_payload.intraday_factor_by_bin.len()
            )
        })?;
    let day_of_week_factor = seasonality_payload
        .day_of_week_factor
        .get(weekday_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "day_of_week factor index {weekday_index} is out of range for payload with {} buckets",
                seasonality_payload.day_of_week_factor.len()
            )
        })?;
    let day_of_year_factor = seasonality_payload
        .day_of_year_factor
        .get(day_of_year_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "day_of_year factor index {day_of_year_index} is out of range for payload with {} buckets",
                seasonality_payload.day_of_year_factor.len()
            )
        })?;

    Ok((intraday_factor * day_of_week_factor * day_of_year_factor).max(1.0e-9))
}

const fn floor_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
    timestamp_utc_nanos.div_euclid(ONE_HOUR_NANOS) * ONE_HOUR_NANOS
}

fn ceil_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
    let floored_timestamp_utc_nanos = floor_to_hour_utc_nanos(timestamp_utc_nanos);
    if floored_timestamp_utc_nanos == timestamp_utc_nanos {
        timestamp_utc_nanos
    } else {
        floored_timestamp_utc_nanos + ONE_HOUR_NANOS
    }
}

fn adjust_candidate_for_deseasonalized_activity(
    candidate_input: &HawkesStageCandidateInput,
    _observed_activity_targets: HawkesObservedActivityTargets,
) -> (HawkesStageCandidateInput, f64) {
    (candidate_input.clone(), 1.0)
}

fn check_seasonality_scaled_stationarity(
    max_seasonality_factor: f64,
    derived_alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    beta: f64,
) -> Option<SeasonalityScaledStationarityCheck> {
    let spectral_radius_alpha_over_beta = spectral_radius_alpha_over_beta(derived_alpha, beta);
    let effective_branching_ratio = max_seasonality_factor * spectral_radius_alpha_over_beta;
    if effective_branching_ratio >= 1.0 {
        return Some(SeasonalityScaledStationarityCheck {
            max_seasonality_factor,
            spectral_radius_alpha_over_beta,
            effective_branching_ratio,
        });
    }

    None
}

fn derive_candidate_implied_metrics(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_hourly_seasonality_factor: f64,
    derived_alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    derived_rho: janiform_model_kernels::hawkes::HawkesKernelLoadings,
) -> HawkesStageImpliedMetricsDiagnostic {
    let scaled_plus_plus = derived_alpha.plus_plus / candidate_input.beta;
    let scaled_plus_minus = derived_alpha.plus_minus / candidate_input.beta;
    let scaled_minus_plus = derived_alpha.minus_plus / candidate_input.beta;
    let scaled_minus_minus = derived_alpha.minus_minus / candidate_input.beta;

    let determinant = (1.0 - scaled_plus_plus) * (1.0 - scaled_minus_minus)
        - scaled_plus_minus * scaled_minus_plus;

    let stationary_buy_multiplier = ((1.0 - scaled_minus_minus) + scaled_plus_minus) / determinant;
    let stationary_sell_multiplier = (scaled_minus_plus + (1.0 - scaled_plus_plus)) / determinant;

    let stationary_buy_intensity = candidate_input.lambda_bar * stationary_buy_multiplier.max(0.0);
    let stationary_sell_intensity =
        candidate_input.lambda_bar * stationary_sell_multiplier.max(0.0);
    let stationary_total_intensity = stationary_buy_intensity + stationary_sell_intensity;

    let model_implied_open_sell_share = if stationary_total_intensity > 0.0 {
        stationary_sell_intensity / stationary_total_intensity
    } else {
        0.5
    };

    let observed_open_event_rate_per_hour =
        observed_open_session_event_rate_per_hour(&candidate_input.source_regime);
    let endogenous_branching_ratio =
        spectral_radius_alpha_over_beta(derived_alpha, candidate_input.beta);
    let max_runtime_effective_branching_ratio =
        max_runtime_hourly_seasonality_factor * endogenous_branching_ratio;
    let self_excitation_ratio =
        (derived_alpha.plus_plus + derived_alpha.minus_minus) / (2.0 * candidate_input.beta);
    let cross_excitation_ratio =
        (derived_alpha.plus_minus + derived_alpha.minus_plus) / (2.0 * candidate_input.beta);
    let exogenous_self_loading_ratio =
        (derived_rho.plus_plus + derived_rho.minus_minus) / (2.0 * candidate_input.beta);
    let exogenous_cross_loading_ratio =
        (derived_rho.plus_minus + derived_rho.minus_plus) / (2.0 * candidate_input.beta);

    let model_implied_interarrival_coefficient_of_variation = (1.0
        + 2.0 * endogenous_branching_ratio / (1.0 - endogenous_branching_ratio).max(1e-9))
    .sqrt();

    let poisson_short_gap_ratio = 1.0
        - (-(stationary_total_intensity / 3600.0)
            * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
            .exp();

    let clustering_decay = (-(candidate_input.beta / 60.0)
        * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
        .exp();

    let model_implied_short_gap_ratio = (poisson_short_gap_ratio
        + (1.0 - poisson_short_gap_ratio) * endogenous_branching_ratio * clustering_decay)
        .clamp(0.0, 0.999_999);

    let news_rate_per_hour = observed_activity_targets.deseasonalized_known_news_rate_per_hour;

    let rho_average_row_sum = (derived_rho.plus_plus
        + derived_rho.plus_minus
        + derived_rho.minus_plus
        + derived_rho.minus_minus)
        / 2.0;

    let exogenous_extra_rate = if news_rate_per_hour > 0.0 {
        news_rate_per_hour * rho_average_row_sum
            / (candidate_input.beta * (1.0 - endogenous_branching_ratio).max(1e-9))
    } else {
        0.0
    };

    let model_implied_known_news_activity_share = if stationary_total_intensity > 0.0 {
        (exogenous_extra_rate / stationary_total_intensity).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let expected_event_count_over_observation_window = stationary_total_intensity
        * candidate_input
            .source_regime
            .open_session_duration_hours
            .max(1.0e-9);

    HawkesStageImpliedMetricsDiagnostic {
        baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
        baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
        observed_open_event_rate_per_hour,
        deseasonalized_open_event_rate_per_hour_target: observed_activity_targets
            .deseasonalized_open_event_rate_per_hour,
        lambda_bar_scaling_ratio_applied,
        max_runtime_hourly_seasonality_factor,
        max_runtime_effective_branching_ratio,
        runtime_effective_branching_margin: (1.0 - max_runtime_effective_branching_ratio).max(0.0),
        model_implied_open_event_rate_per_hour: stationary_total_intensity,
        model_implied_open_buy_share: 1.0 - model_implied_open_sell_share,
        model_implied_open_sell_share,
        model_implied_interarrival_coefficient_of_variation,
        model_implied_short_gap_ratio,
        model_implied_known_news_activity_share,
        model_implied_expected_event_count_over_observation_window:
            expected_event_count_over_observation_window,
        endogenous_branching_ratio,
        self_excitation_ratio,
        cross_excitation_ratio,
        exogenous_self_loading_ratio,
        exogenous_cross_loading_ratio,
        stability_margin: (1.0 - endogenous_branching_ratio).max(0.0),
        event_volume_ratio_vs_total_observed: expected_event_count_over_observation_window
            / (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
    }
}

fn push_objective_term(
    term_id: &str,
    weight: f64,
    observed_value: f64,
    model_implied_value: f64,
    scale: f64,
    objective_terms: &mut Vec<CalibrationObjectiveTermInput>,
    term_diagnostics: &mut Vec<HawkesStageObjectiveTermDiagnostic>,
) {
    let absolute_error = (model_implied_value - observed_value).abs();
    let normalized_error = absolute_error / scale.max(1e-9);
    let weighted_contribution = weight * normalized_error;

    objective_terms.push(CalibrationObjectiveTermInput {
        term_id: term_id.to_string(),
        weight,
        observed_score: normalized_error,
    });
    term_diagnostics.push(HawkesStageObjectiveTermDiagnostic {
        term_id: term_id.to_string(),
        weight,
        observed_value,
        model_implied_value,
        scale,
        absolute_error,
        normalized_error,
        weighted_contribution,
    });
}

fn observed_open_session_event_rate_per_hour(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    source_regime.open_session_tick_count as f64
        / source_regime.open_session_duration_hours.max(1e-9)
}

fn observed_directional_imbalance_magnitude(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    ((source_regime.open_session_sell_tick_count as f64
        / source_regime.open_session_tick_count as f64)
        - 0.5)
        .abs()
        * 2.0
}

fn spectral_radius_alpha_over_beta(
    alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    beta: f64,
) -> f64 {
    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;
    let trace = scaled_plus_plus + scaled_minus_minus;
    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .sqrt();

    0.5 * (trace + discriminant)
}

fn hawkes_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![
        CalibrationOptimizerParameterBound {
            parameter_name: "lambda_bar".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "beta".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "stability_numeric_tolerance".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
    ]
}

fn build_hawkes_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<HawkesStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Hawkes)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => HawkesStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport(),
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                HawkesStageHandoffOutput {
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

fn build_hawkes_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: HawkesStageExecutionFailure,
) -> HawkesStageExecutionReport {
    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
