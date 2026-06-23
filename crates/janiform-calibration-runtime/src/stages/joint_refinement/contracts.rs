use serde::{Deserialize, Serialize};

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy;
use janiform_engine_pricing::{PricingIntervalSchedulePolicy, VanillaOptionPayoffType};
use janiform_model_kernels::hawkes::HawkesEventSide;

use crate::diagnostics::types::CalibrationDiagnosticsRequest;
use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
use crate::stages::progress::progress_percent;
use crate::stages::return_components::{
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageFittedPayload,
};
use crate::stages::seasonality::{CalibrationStageExecutionStatus, FourierBusinessTimeClockModel};
use janiform_calibration_artifacts::{
    CalibrationEffortOptions, CalibrationExecutionMode, CalibrationLossTermDiagnostic,
    CalibrationOptimizerSettings, CalibrationStage, CalibrationStagePayloadSource,
    CalibrationStageSeedSource, JointRefinementStageCarryInput,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
    JointRefinementStageOptimizerDiagnostics, JointRefinementStageOptionCalibrationDiagnostics,
    JointRefinementStageRestartEvaluationTimingDiagnostic, JointRefinementStageRuntimeProfile,
};

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageSeasonalityFactorsInput {
    pub exchange_code: String,
    pub clock_model: FourierBusinessTimeClockModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageCurrentStateEstimationInput {
    pub state_time_utc_nanos: i64,
    pub reference_price: f64,
    pub observed_spot: f64,
    pub observed_log_return: f64,
    pub observation_timestamp_utc_nanos: Vec<i64>,
    pub observation_trade_prices: Vec<f64>,
    pub observation_trade_sides: Vec<HawkesEventSide>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageOptionSnapshotNode {
    pub quote_timestamp_utc_nanos: i64,
    pub expiry_timestamp_utc_nanos: i64,
    pub strike: f64,
    pub payoff_type: VanillaOptionPayoffType,
    pub bid_price: f64,
    pub ask_price: f64,
    pub observed_mid_price: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageOptionPricingInput {
    pub valuation_time_utc_nanos: i64,
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    pub carry_input: JointRefinementStageCarryInput,
    pub option_valuation_spot: f64,
    pub current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput,
    pub option_snapshot_nodes: Vec<JointRefinementStageOptionSnapshotNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageExecutionInput {
    pub seasonality_factors_input: JointRefinementStageSeasonalityFactorsInput,
    pub hawkes_input: CalibrationStagePayloadSource<HawkesStageFittedPayload>,
    pub return_components_input: CalibrationStagePayloadSource<ReturnComponentsStageFittedPayload>,
    pub mean_reversion_input: CalibrationStagePayloadSource<MeanReversionStageFittedPayload>,
    pub candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    pub coarse_data_records: Option<Vec<MarketDataNormalizedCoarseDataRecord>>,
    pub tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
    pub calibration_effort: Option<CalibrationEffortOptions>,
    pub restart_candidate_budget_override: Option<usize>,
    pub treat_invalid_restart_fallback_warning_as_error: bool,
    pub option_refinement_candidate_count_override: Option<usize>,
    pub option_refinement_wall_clock_budget_millis: Option<u64>,
    pub audit_option_pricing_enabled: bool,
    pub diagnostics: CalibrationDiagnosticsRequest,
    pub option_pricing_input: Option<JointRefinementStageOptionPricingInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JointRefinementStageProgressPhase {
    StageInputResolution,
    ResumeStateResolution,
    ObservedInputPreparation,
    ParameterizationPreparation,
    RuntimeMaterialization,
    RuntimeContextAssembly,
    BaselineLocalEvaluation,
    BaselineFinalEvaluation,
    BaselineCoarseConsistencyEvaluation,
    RestartSeedGeneration,
    RestartCandidateScreening,
    RestartOptimization,
    CoarseConsistencyEvaluation,
    MarketPressureRefit,
    OptionSearchTierRefinement,
    OptionSelectionTierEvaluation,
    OptionAuditTierEvaluation,
    FinalSeedEvaluation,
    FinalAssembly,
}

#[cfg(test)]
const JOINT_REFINEMENT_PROGRESS_PHASES: &[JointRefinementStageProgressPhase] = &[
    JointRefinementStageProgressPhase::StageInputResolution,
    JointRefinementStageProgressPhase::ResumeStateResolution,
    JointRefinementStageProgressPhase::ObservedInputPreparation,
    JointRefinementStageProgressPhase::ParameterizationPreparation,
    JointRefinementStageProgressPhase::RuntimeMaterialization,
    JointRefinementStageProgressPhase::RuntimeContextAssembly,
    JointRefinementStageProgressPhase::BaselineLocalEvaluation,
    JointRefinementStageProgressPhase::BaselineFinalEvaluation,
    JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
    JointRefinementStageProgressPhase::RestartSeedGeneration,
    JointRefinementStageProgressPhase::RestartCandidateScreening,
    JointRefinementStageProgressPhase::RestartOptimization,
    JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
    JointRefinementStageProgressPhase::MarketPressureRefit,
    JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
    JointRefinementStageProgressPhase::OptionAuditTierEvaluation,
    JointRefinementStageProgressPhase::FinalSeedEvaluation,
    JointRefinementStageProgressPhase::FinalAssembly,
];

// UI-facing stage progress contract. Units describe the expected share of the
// joint-refinement stage for each phase and must cover every progress phase.
// Single-pass setup phases use one unit because they expose start/completion
// boundaries without an internal work budget.
const JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS: usize = 1;
const JOINT_REFINEMENT_PROGRESS_WEIGHT_UNITS_V1: &[(JointRefinementStageProgressPhase, usize)] = &[
    (
        JointRefinementStageProgressPhase::StageInputResolution,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::ResumeStateResolution,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::ObservedInputPreparation,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::ParameterizationPreparation,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::RuntimeMaterialization,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::RuntimeContextAssembly,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::BaselineLocalEvaluation,
        4,
    ),
    (
        JointRefinementStageProgressPhase::BaselineFinalEvaluation,
        8,
    ),
    (
        JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
        4,
    ),
    (JointRefinementStageProgressPhase::RestartSeedGeneration, 4),
    (
        JointRefinementStageProgressPhase::RestartCandidateScreening,
        4,
    ),
    (JointRefinementStageProgressPhase::RestartOptimization, 56),
    (
        JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
        4,
    ),
    (
        JointRefinementStageProgressPhase::MarketPressureRefit,
        JOINT_REFINEMENT_SINGLE_PASS_SETUP_WEIGHT_UNITS,
    ),
    (
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        8,
    ),
    (
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        4,
    ),
    (
        JointRefinementStageProgressPhase::OptionAuditTierEvaluation,
        2,
    ),
    (JointRefinementStageProgressPhase::FinalSeedEvaluation, 1),
    (JointRefinementStageProgressPhase::FinalAssembly, 1),
];

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageProgressUpdate {
    pub phase: JointRefinementStageProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub aggregate_progress_percent: Option<usize>,
    pub current_item_label: Option<String>,
    pub current_restart_id: Option<String>,
    pub current_work_item_status: Option<String>,
    pub planned_work_item_labels: Vec<String>,
    pub active_restart_count: Option<usize>,
    pub completed_outer_cycles: Option<usize>,
    pub total_outer_cycles: Option<usize>,
    pub current_evaluation_count: Option<usize>,
    pub total_evaluation_budget: Option<usize>,
    pub evaluation_progress_percent: Option<usize>,
    pub current_pricing_interval_index: Option<usize>,
    pub total_pricing_interval_count: Option<usize>,
    pub completed_pricing_row_count: Option<usize>,
    pub total_pricing_row_count: Option<usize>,
    pub valid_completed_units: Option<usize>,
    pub cumulative_local_evaluation_count: Option<usize>,
    pub current_global_loss: Option<f64>,
    pub current_restart_best_global_loss: Option<f64>,
    pub current_final_global_loss: Option<f64>,
    pub current_candidate_assessment: Option<JointRefinementProgressCandidateAssessment>,
    pub best_restart_id: Option<String>,
    pub best_global_loss: Option<f64>,
    pub current_coarse_consistency_fit: Option<f64>,
    pub best_coarse_consistency_fit: Option<f64>,
    pub exact_closed_form_buy_count: Option<usize>,
    pub exact_closed_form_sell_count: Option<usize>,
    pub exact_tilted_mixed_count: Option<usize>,
    pub restart_evaluation_timing_diagnostic:
        Option<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

impl JointRefinementStageProgressUpdate {
    pub(crate) fn new(
        phase: JointRefinementStageProgressPhase,
        completed_units: usize,
        total_units: usize,
    ) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
            aggregate_progress_percent: None,
            current_item_label: None,
            current_restart_id: None,
            current_work_item_status: None,
            planned_work_item_labels: Vec::new(),
            active_restart_count: None,
            completed_outer_cycles: None,
            total_outer_cycles: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
            current_pricing_interval_index: None,
            total_pricing_interval_count: None,
            completed_pricing_row_count: None,
            total_pricing_row_count: None,
            valid_completed_units: None,
            cumulative_local_evaluation_count: None,
            current_global_loss: None,
            current_restart_best_global_loss: None,
            current_final_global_loss: None,
            current_candidate_assessment: None,
            best_restart_id: None,
            best_global_loss: None,
            current_coarse_consistency_fit: None,
            best_coarse_consistency_fit: None,
            exact_closed_form_buy_count: None,
            exact_closed_form_sell_count: None,
            exact_tilted_mixed_count: None,
            restart_evaluation_timing_diagnostic: None,
        }
    }

    pub(crate) fn stage_progress_percent(&self) -> usize {
        joint_refinement_stage_progress_percent(
            self.phase,
            self.completed_units,
            self.total_units,
            self.aggregate_progress_percent,
        )
    }
}

/// User-facing classification for a joint-refinement candidate progress row.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointRefinementProgressCandidateAssessment {
    /// Candidate is currently the leading restart among admissible candidates.
    Champion,
    /// Candidate completed final checks inside the admissibility contract.
    Admissible,
    /// Candidate completed but missed at least one admissibility threshold.
    Inadmissible,
    /// Candidate could not produce a valid local or final evaluation.
    Invalid,
}

fn joint_refinement_stage_progress_percent(
    phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    aggregate_progress_percent: Option<usize>,
) -> usize {
    let total_weight_units = JOINT_REFINEMENT_PROGRESS_WEIGHT_UNITS_V1
        .iter()
        .map(|(_, weight_units)| *weight_units)
        .sum::<usize>();
    if total_weight_units == 0 {
        return 0;
    }

    let mut completed_weight_units = 0usize;

    for (progress_phase, weight_units) in JOINT_REFINEMENT_PROGRESS_WEIGHT_UNITS_V1 {
        if *progress_phase == phase {
            let phase_progress_percent = aggregate_progress_percent
                .unwrap_or_else(|| progress_percent(completed_units, total_units))
                .min(100);

            let progress_units =
                completed_weight_units.saturating_mul(100) + weight_units * phase_progress_percent;

            return progress_units
                .checked_div(total_weight_units)
                .unwrap_or(0)
                .min(100);
        }

        completed_weight_units = completed_weight_units.saturating_add(*weight_units);
    }

    0
}

#[cfg(test)]
mod tests {
    use super::{
        JOINT_REFINEMENT_PROGRESS_PHASES, JOINT_REFINEMENT_PROGRESS_WEIGHT_UNITS_V1,
        JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
    };

    #[test]
    fn joint_refinement_stage_progress_weight_policy_covers_all_progress_phases() {
        for phase in JOINT_REFINEMENT_PROGRESS_PHASES {
            assert!(
                JOINT_REFINEMENT_PROGRESS_WEIGHT_UNITS_V1
                    .iter()
                    .any(|(weighted_phase, _)| weighted_phase == phase)
            );
        }
    }

    #[test]
    fn joint_refinement_stage_progress_orders_baseline_coarse_before_restart_seed_generation() {
        let baseline_coarse_progress = JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
            1,
            1,
        )
        .stage_progress_percent();

        let restart_seed_progress = JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartSeedGeneration,
            0,
            1,
        )
        .stage_progress_percent();

        assert_eq!(baseline_coarse_progress, 20);
        assert_eq!(restart_seed_progress, 20);
    }

    #[test]
    fn joint_refinement_stage_progress_uses_restart_aggregate_percent() {
        let mut progress_update = JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartOptimization,
            18,
            22,
        );

        progress_update.aggregate_progress_percent = Some(2);

        assert_eq!(progress_update.stage_progress_percent(), 29);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters:
        janiform_calibration_artifacts::HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters:
        janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
    pub candidate_frontier: Vec<JointRefinementStageCandidateDiagnostic>,
    pub simulation_summaries: Vec<JointRefinementStageSimulationSummary>,
    pub coarse_tick_consistency: Vec<JointRefinementStageCoarseTickConsistencyMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_calibration_diagnostics: Option<JointRefinementStageOptionCalibrationDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageCandidateDiagnostic {
    pub candidate_id: String,
    pub valid: bool,
    pub admissible: bool,
    pub pareto_rank: usize,
    pub lexicographic_rank: usize,
    pub scalar_tie_break: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    pub objective_terms: Vec<JointRefinementStageObjectiveTermDiagnostic>,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub canonical_coordinates: Vec<(String, f64)>,
}

pub type JointRefinementStageObjectiveTermDiagnostic = CalibrationLossTermDiagnostic;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageSimulationSummary {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub event_count: usize,
    pub realized_variance: f64,
    pub mean_absolute_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageCoarseTickConsistencyMetric {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub metric_id: String,
    pub coarse_value: f64,
    pub tick_value: f64,
    pub absolute_delta: f64,
    pub normalized_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionMetadata {
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
pub enum JointRefinementStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingSeasonalityPayload,
    MissingHawkesPayload,
    MissingReturnComponentsPayload,
    MissingMeanReversionPayload,
    InvalidCandidateGenerationInput { error_message: String },
    SimulationInfrastructureUnavailable { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<JointRefinementStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JointRefinementStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_profile: Option<JointRefinementStageRuntimeProfile>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<JointRefinementStageExecutionFailure>,
    #[serde(skip)]
    pub(crate) runtime_handoff: Option<super::execution::JointRefinementStageRuntimeHandoff>,
}
