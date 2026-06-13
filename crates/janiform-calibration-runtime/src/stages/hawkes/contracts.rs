use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::progress::StageProgressUpdate;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
};
use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource, HawkesStageFittedParameters,
};
use janiform_foundations::time::SECONDS_PER_HOUR;
use janiform_model_kernels::hawkes::{
    HawkesKernelParameterValidationReport, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
};

/// Compact event-side code for a buy-side Hawkes event.
pub const HAWKES_EVENT_SIDE_CODE_BUY: u8 = 1;

/// Compact event-side code for a sell-side Hawkes event.
pub const HAWKES_EVENT_SIDE_CODE_SELL: u8 = 2;

// One hour is the existing continuity boundary for the open-event Hawkes state recursion.
pub(super) const HAWKES_MAX_CONTINUOUS_OPEN_GAP_SECONDS: f64 = SECONDS_PER_HOUR;

// Seasonality factors are positive by construction, but persisted artifacts can
// contain machine-scale edge values near sparse windows.
pub(super) const HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR: f64 = 1.0e-9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub(super) fn validate(&self) -> Result<(), HawkesStageObservedSourceRegimeError> {
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
    pub(super) fn validate(&self) -> Result<(), HawkesStageDirectionalLabelDiagnosticsError> {
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
    pub(super) fn validate(&self) -> Result<(), HawkesStageCandidateInputError> {
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
    pub(super) fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub(super) fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference),
        }
    }

    pub(super) fn dataset_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.dataset_id.as_str()),
            Self::PersistedArtifactReference { .. } => None,
        }
    }

    pub(super) fn payload(&self) -> &SeasonalityStageNormalizedPayload {
        match self {
            Self::InMemoryOutput { payload } => payload,
            Self::PersistedArtifactReference { payload, .. } => payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageExecutionInput {
    pub seasonality_input: HawkesStageSeasonalityInput,
    pub candidate_inputs: Vec<HawkesStageCandidateInput>,
    pub event_side_codes: Vec<u8>,
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
            event_side_codes: Vec::new(),
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
            event_side_codes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesStageProgressPhase {
    InputPreflight,
    EventTapePreparation,
    CandidatePreparation,
    CandidateEvaluation,
    CandidateRefinement,
    LikelihoodShortlistEvaluation,
    CandidateSelection,
    ReportAssembly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageProgressUpdate {
    pub phase: HawkesStageProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub aggregate_progress_percent: Option<usize>,
    pub current_item_label: Option<String>,
    pub current_evaluation_count: Option<usize>,
    pub total_evaluation_budget: Option<usize>,
    pub evaluation_progress_percent: Option<usize>,
    pub current_weighted_objective: Option<f64>,
    pub best_weighted_objective: Option<f64>,
    pub current_convergence_ratio: Option<f64>,
}

impl HawkesStageProgressUpdate {
    pub(crate) fn new(
        phase: HawkesStageProgressPhase,
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
            current_weighted_objective: None,
            best_weighted_objective: None,
            current_convergence_ratio: None,
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
    pub max_runtime_interval_seasonality_factor: f64,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

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
    InvalidSeasonalityPayload {
        error_message: String,
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
