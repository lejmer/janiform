use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::strings::is_non_empty_string;
use janiform_foundations::time::duration_nanos_to_hours;
use serde::{Deserialize, Serialize};

use crate::objective::{
    CalibrationObjectiveTermDiagnostic, HawkesStageObjectiveTermInput,
    HawkesStageObjectiveTermInputError,
};
use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    ReturnComponentsStageFittedParameters, ReturnMarkHourOfWeekScaleProfile,
    TickGridObservationModel,
};
use janiform_market_time::calendar::MarketState;
use janiform_market_time::seasonality::HOURS_PER_WEEK;
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::constants::{
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT,
};

/// One weighted objective term used by return-components-stage candidate evaluation.
pub type ReturnComponentsStageObjectiveTermInput = HawkesStageObjectiveTermInput;

/// One weighted objective-term diagnostic emitted by return-components-stage evaluation.
pub type ReturnComponentsStageObjectiveTermDiagnostic = CalibrationObjectiveTermDiagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnComponentsStageProgressPhase {
    InputPreflight,
    CloseOpenGapPreparation,
    SignalPartitionFit,
    SurpriseEpisodeSelectionFit,
    CandidateInputValidation,
    SignalSorting,
    SignalSummary,
    QuantileBinningPolicyFit,
    SurpriseArrivalFit,
    ReturnJumpMarkFit,
    ActivityCouplingFit,
    ObjectivePolicyFit,
    ContinuousReturnLayerFit,
    ContinuousNigMomentSeedGeneration,
    ContinuousNigProxyEmSeedGeneration,
    ContinuousNigSeedScoring,
    ContinuousNigSearchRegimeGeneration,
    ContinuousNigBasinSeedGeneration,
    ContinuousNigBasinOptimization,
    ContinuousNigBasinDiagnostics,
    ContinuousNigFinalEvaluation,
    CandidateEnumeration,
    CandidateConstraintEvaluation,
    CandidateSelection,
    ReportAssembly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageProgressUpdate {
    pub phase: ReturnComponentsStageProgressPhase,
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

impl ReturnComponentsStageProgressUpdate {
    pub(crate) fn new(
        phase: ReturnComponentsStageProgressPhase,
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
}

/// Canonical source window for candidate-generation signal extraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsStageSignalSourceWindow {
    pub window_start_utc_nanos: i64,
    pub window_end_utc_nanos: i64,
}

impl ReturnComponentsStageSignalSourceWindow {
    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.window_end_utc_nanos <= self.window_start_utc_nanos {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::NonIncreasingSourceWindow {
                    window_start_utc_nanos: self.window_start_utc_nanos,
                    window_end_utc_nanos: self.window_end_utc_nanos,
                },
            );
        }

        Ok(())
    }

    pub(super) fn duration_hours(self) -> f64 {
        let duration_nanos = self
            .window_end_utc_nanos
            .saturating_sub(self.window_start_utc_nanos);
        duration_nanos_to_hours(duration_nanos)
    }
}

/// Known-news calendar anchors used by candidate generation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsKnownNewsCalendarInput {
    pub source_window: ReturnComponentsStageSignalSourceWindow,
    pub scheduled_event_timestamps_utc_nanos: Vec<i64>,
}

impl ReturnComponentsKnownNewsCalendarInput {
    pub(super) fn validate(
        &self,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        self.source_window.validate()?;

        let known_arrival_configuration = KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: self.scheduled_event_timestamps_utc_nanos.clone(),
        };

        known_arrival_configuration.validate().map_err(|source| {
            ReturnComponentsStageCandidateGenerationInputError::InvalidKnownNewsCalendar {
                error_message: source.to_string(),
            }
        })?;

        for &timestamp_utc_nanos in &self.scheduled_event_timestamps_utc_nanos {
            if timestamp_utc_nanos < self.source_window.window_start_utc_nanos
                || timestamp_utc_nanos >= self.source_window.window_end_utc_nanos
            {
                return Err(
                    ReturnComponentsStageCandidateGenerationInputError::KnownNewsEventOutsideSourceWindow {
                        timestamp_utc_nanos,
                        window_start_utc_nanos: self.source_window.window_start_utc_nanos,
                        window_end_utc_nanos: self.source_window.window_end_utc_nanos,
                    },
                );
            }
        }

        Ok(())
    }
}

/// Realized known-news return sample aligned to one known calendar anchor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsRealizedKnownNewsReturnSample {
    pub event_timestamp_utc_nanos: i64,
    pub attributed_open_return_timestamp_utc_nanos: Option<i64>,
    pub open_window_jump_return: f64,
    pub closed_window_gap_return: f64,
    pub post_shock_activity_lift: f64,
}

impl ReturnComponentsRealizedKnownNewsReturnSample {
    pub(super) fn validate(
        self,
        source_window: ReturnComponentsStageSignalSourceWindow,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.event_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || self.event_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::KnownNewsReturnSampleOutsideSourceWindow {
                    event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                    window_start_utc_nanos: source_window.window_start_utc_nanos,
                    window_end_utc_nanos: source_window.window_end_utc_nanos,
                },
            );
        }

        if let Some(attributed_open_return_timestamp_utc_nanos) =
            self.attributed_open_return_timestamp_utc_nanos
        {
            if attributed_open_return_timestamp_utc_nanos < source_window.window_start_utc_nanos
                || attributed_open_return_timestamp_utc_nanos >= source_window.window_end_utc_nanos
            {
                return Err(
                    ReturnComponentsStageCandidateGenerationInputError::KnownNewsAttributedOpenReturnOutsideSourceWindow {
                        event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                        attributed_open_return_timestamp_utc_nanos,
                        window_start_utc_nanos: source_window.window_start_utc_nanos,
                        window_end_utc_nanos: source_window.window_end_utc_nanos,
                    },
                );
            }

            if attributed_open_return_timestamp_utc_nanos < self.event_timestamp_utc_nanos {
                return Err(
                    ReturnComponentsStageCandidateGenerationInputError::KnownNewsAttributedOpenReturnPrecedesEvent {
                        event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                        attributed_open_return_timestamp_utc_nanos,
                    },
                );
            }
        }

        if !self.open_window_jump_return.is_finite() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidKnownNewsOpenJumpReturn {
                    event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                    value: self.open_window_jump_return,
                },
            );
        }

        if !self.closed_window_gap_return.is_finite() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidKnownNewsClosedGapReturn {
                    event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                    value: self.closed_window_gap_return,
                },
            );
        }

        if !self.post_shock_activity_lift.is_finite() || self.post_shock_activity_lift < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidKnownNewsActivityLift {
                    event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
                    value: self.post_shock_activity_lift,
                },
            );
        }

        Ok(())
    }
}

/// Observed same-session open-window return used for NIG and jump evidence fitting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsObservedOpenSessionReturn {
    pub return_timestamp_utc_nanos: i64,
    pub elapsed_same_session_hours: f64,
    pub log_return: f64,
    pub post_shock_activity_lift: f64,
    pub sell_share_proxy: f64,
}

impl ReturnComponentsObservedOpenSessionReturn {
    pub(super) fn validate(
        self,
        source_window: ReturnComponentsStageSignalSourceWindow,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.return_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || self.return_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::ObservedOpenSessionReturnOutsideSourceWindow {
                    return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
                    window_start_utc_nanos: source_window.window_start_utc_nanos,
                    window_end_utc_nanos: source_window.window_end_utc_nanos,
                },
            );
        }

        if !self.log_return.is_finite() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
                    value: self.log_return,
                },
            );
        }

        if !self.elapsed_same_session_hours.is_finite() || self.elapsed_same_session_hours <= 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidObservedOpenSessionReturnElapsedTime {
                    return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
                    value: self.elapsed_same_session_hours,
                },
            );
        }

        if !self.post_shock_activity_lift.is_finite() || self.post_shock_activity_lift < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidObservedOpenSessionActivityLift {
                    return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
                    value: self.post_shock_activity_lift,
                },
            );
        }

        if !self.sell_share_proxy.is_finite() || !(0.0..=1.0).contains(&self.sell_share_proxy) {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidObservedOpenSessionSellShareProxy {
                    return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
                    value: self.sell_share_proxy,
                },
            );
        }

        Ok(())
    }
}

/// Detected intraday jump candidate from observed open-window returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsDetectedIntradayJumpCandidate {
    pub jump_timestamp_utc_nanos: i64,
    pub jump_return: f64,
    pub post_shock_activity_lift: f64,
}

impl ReturnComponentsDetectedIntradayJumpCandidate {
    pub(super) fn validate(
        self,
        source_window: ReturnComponentsStageSignalSourceWindow,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.jump_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || self.jump_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::IntradayJumpOutsideSourceWindow {
                    jump_timestamp_utc_nanos: self.jump_timestamp_utc_nanos,
                    window_start_utc_nanos: source_window.window_start_utc_nanos,
                    window_end_utc_nanos: source_window.window_end_utc_nanos,
                },
            );
        }

        if !self.jump_return.is_finite() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidIntradayJumpReturn {
                    jump_timestamp_utc_nanos: self.jump_timestamp_utc_nanos,
                    value: self.jump_return,
                },
            );
        }

        if !self.post_shock_activity_lift.is_finite() || self.post_shock_activity_lift < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidIntradayActivityLift {
                    jump_timestamp_utc_nanos: self.jump_timestamp_utc_nanos,
                    value: self.post_shock_activity_lift,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReturnComponentsSurpriseJumpEpisode {
    pub(crate) member_jump_timestamps_utc_nanos: Vec<i64>,
    pub(crate) terminal_timestamp_utc_nanos: i64,
    pub(crate) cumulative_jump_return: f64,
    pub(crate) post_shock_activity_lift: f64,
}

/// Detected close-open gap candidate from observed close-open boundaries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsDetectedCloseOpenGapCandidate {
    pub close_timestamp_utc_nanos: i64,
    pub open_timestamp_utc_nanos: i64,
    pub gap_return: f64,
    pub post_shock_activity_lift: f64,
}

impl ReturnComponentsDetectedCloseOpenGapCandidate {
    pub(super) fn validate(
        self,
        source_window: ReturnComponentsStageSignalSourceWindow,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.close_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || self.open_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::CloseOpenGapOutsideSourceWindow {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    window_start_utc_nanos: source_window.window_start_utc_nanos,
                    window_end_utc_nanos: source_window.window_end_utc_nanos,
                },
            );
        }

        if self.open_timestamp_utc_nanos <= self.close_timestamp_utc_nanos {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::NonIncreasingCloseOpenGapWindow {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                },
            );
        }

        if !self.gap_return.is_finite() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidCloseOpenGapReturn {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    value: self.gap_return,
                },
            );
        }

        if !self.post_shock_activity_lift.is_finite() || self.post_shock_activity_lift < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidCloseOpenActivityLift {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    value: self.post_shock_activity_lift,
                },
            );
        }

        Ok(())
    }
}

/// Raw observed close-open boundary input used to derive material gap candidates internally.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsObservedCloseOpenBoundaryObservation {
    pub close_timestamp_utc_nanos: i64,
    pub close_price: f64,
    pub open_timestamp_utc_nanos: i64,
    pub open_price: f64,
    pub post_shock_activity_lift: f64,
}

impl ReturnComponentsObservedCloseOpenBoundaryObservation {
    pub(super) fn validate(
        self,
        source_window: ReturnComponentsStageSignalSourceWindow,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if self.close_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || self.open_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::CloseOpenBoundaryOutsideSourceWindow {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    window_start_utc_nanos: source_window.window_start_utc_nanos,
                    window_end_utc_nanos: source_window.window_end_utc_nanos,
                },
            );
        }

        if self.open_timestamp_utc_nanos <= self.close_timestamp_utc_nanos {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::NonIncreasingCloseOpenBoundaryWindow {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                },
            );
        }

        if !self.close_price.is_finite() || self.close_price <= 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryPrice {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    value: self.close_price,
                },
            );
        }

        if !self.open_price.is_finite() || self.open_price <= 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidOpenBoundaryPrice {
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    value: self.open_price,
                },
            );
        }

        if !self.post_shock_activity_lift.is_finite() || self.post_shock_activity_lift < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidCloseOpenBoundaryActivityLift {
                    close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
                    open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
                    value: self.post_shock_activity_lift,
                },
            );
        }

        Ok(())
    }
}

/// Explicit materiality rule for deriving close-open gap candidates from raw observations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsCloseOpenGapDetectionPolicy {
    pub material_gap_absolute_return_tolerance: f64,
}

impl ReturnComponentsCloseOpenGapDetectionPolicy {
    pub const fn new(material_gap_absolute_return_tolerance: f64) -> Self {
        Self {
            material_gap_absolute_return_tolerance,
        }
    }

    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if !self.material_gap_absolute_return_tolerance.is_finite()
            || self.material_gap_absolute_return_tolerance < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidCloseOpenGapMaterialityTolerance {
                    value: self.material_gap_absolute_return_tolerance,
                },
            );
        }

        Ok(())
    }
}

/// Objective metric-bucket weights for candidate generation and evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsStageObjectiveMetricWeights {
    pub continuous_distribution_fit_weight: f64,
    pub surprise_arrival_fit_weight: f64,
    pub jump_fit_weight: f64,
    pub gap_fit_weight: f64,
    pub activity_coupling_fit_weight: f64,
    pub price_range_sanity_fit_weight: f64,
}

impl ReturnComponentsStageObjectiveMetricWeights {
    pub const fn new(
        continuous_distribution_fit_weight: f64,
        surprise_arrival_fit_weight: f64,
        jump_fit_weight: f64,
        gap_fit_weight: f64,
        activity_coupling_fit_weight: f64,
        price_range_sanity_fit_weight: f64,
    ) -> Self {
        Self {
            continuous_distribution_fit_weight,
            surprise_arrival_fit_weight,
            jump_fit_weight,
            gap_fit_weight,
            activity_coupling_fit_weight,
            price_range_sanity_fit_weight,
        }
    }

    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if !self.continuous_distribution_fit_weight.is_finite()
            || self.continuous_distribution_fit_weight < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidContinuousDistributionFitWeight {
                    value: self.continuous_distribution_fit_weight,
                },
            );
        }

        if !self.surprise_arrival_fit_weight.is_finite() || self.surprise_arrival_fit_weight < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidSurpriseArrivalFitWeight {
                    value: self.surprise_arrival_fit_weight,
                },
            );
        }

        if !self.jump_fit_weight.is_finite() || self.jump_fit_weight < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidOpenJumpFitWeight {
                    value: self.jump_fit_weight,
                },
            );
        }

        if !self.gap_fit_weight.is_finite() || self.gap_fit_weight < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidClosedGapFitWeight {
                    value: self.gap_fit_weight,
                },
            );
        }

        if !self.activity_coupling_fit_weight.is_finite() || self.activity_coupling_fit_weight < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidActivityCouplingFitWeight {
                    value: self.activity_coupling_fit_weight,
                },
            );
        }

        if !self.price_range_sanity_fit_weight.is_finite()
            || self.price_range_sanity_fit_weight < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidPriceRangeSanityFitWeight {
                    value: self.price_range_sanity_fit_weight,
                },
            );
        }

        Ok(())
    }
}

/// Canonical candidate-generation payload derived from observed return and activity signals.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageCandidateGenerationInput {
    pub known_news_calendar: ReturnComponentsKnownNewsCalendarInput,
    pub realized_known_news_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    pub observed_open_session_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    pub detected_intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    pub observed_close_open_boundary_observations:
        Vec<ReturnComponentsObservedCloseOpenBoundaryObservation>,
    pub close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy,
    pub open_session_sell_share_proxy: f64,
    pub shared_mark_law_template: SharedNewsMarkLawConfiguration,
    pub return_mark_hour_of_week_scale_profile: ReturnMarkHourOfWeekScaleProfile,
    pub tick_grid_observation_model: TickGridObservationModel,
    pub objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
}

impl ReturnComponentsStageCandidateGenerationInput {
    pub(super) fn validate(
        &self,
    ) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        self.known_news_calendar.validate()?;

        if self.observed_open_session_returns.is_empty() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::MissingObservedOpenSessionReturns,
            );
        }

        for sample in &self.realized_known_news_return_samples {
            sample.validate(self.known_news_calendar.source_window)?;
        }

        for observed_return in &self.observed_open_session_returns {
            observed_return.validate(self.known_news_calendar.source_window)?;
        }

        for jump_candidate in &self.detected_intraday_jump_candidates {
            jump_candidate.validate(self.known_news_calendar.source_window)?;
        }

        for boundary_observation in &self.observed_close_open_boundary_observations {
            boundary_observation.validate(self.known_news_calendar.source_window)?;
        }

        self.close_open_gap_detection_policy.validate()?;
        if !self.open_session_sell_share_proxy.is_finite()
            || !(0.0..=1.0).contains(&self.open_session_sell_share_proxy)
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidOpenSessionSellShareProxy {
                    value: self.open_session_sell_share_proxy,
                },
            );
        }

        self.shared_mark_law_template.validate().map_err(|source| {
            ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
                error_message: source.to_string(),
            }
        })?;

        validate_return_mark_hour_of_week_scale_profile(
            &self.return_mark_hour_of_week_scale_profile,
        )?;
        validate_tick_grid_observation_model(&self.tick_grid_observation_model)?;
        self.objective_metric_weights.validate()?;

        Ok(())
    }
}

fn validate_return_mark_hour_of_week_scale_profile(
    profile: &ReturnMarkHourOfWeekScaleProfile,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    if profile.hour_of_week_factors.len() != HOURS_PER_WEEK {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::ReturnMarkHourOfWeekFactorCountMismatch {
                expected_factor_count: HOURS_PER_WEEK,
                observed_factor_count: profile.hour_of_week_factors.len(),
            },
        );
    }

    for (factor_index, factor) in profile.hour_of_week_factors.iter().copied().enumerate() {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidReturnMarkHourOfWeekFactor {
                    factor_index,
                    value: factor,
                },
            );
        }
    }

    if profile.profile_resolution_hours != 1 {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidReturnMarkProfileResolution {
                value: profile.profile_resolution_hours,
            },
        );
    }

    Ok(())
}

fn validate_tick_grid_observation_model(
    model: &TickGridObservationModel,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    if let Some(tick_size) = model.tick_size
        && (!tick_size.is_finite() || tick_size <= 0.0)
    {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidTickSize {
                value: tick_size,
            },
        );
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageCandidateGenerationInputError {
    NonIncreasingSourceWindow {
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    InvalidKnownNewsCalendar {
        error_message: String,
    },
    KnownNewsEventOutsideSourceWindow {
        timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    MissingObservedOpenSessionReturns,
    KnownNewsReturnSampleOutsideSourceWindow {
        event_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    KnownNewsAttributedOpenReturnOutsideSourceWindow {
        event_timestamp_utc_nanos: i64,
        attributed_open_return_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    KnownNewsAttributedOpenReturnPrecedesEvent {
        event_timestamp_utc_nanos: i64,
        attributed_open_return_timestamp_utc_nanos: i64,
    },
    InvalidKnownNewsOpenJumpReturn {
        event_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidKnownNewsClosedGapReturn {
        event_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidKnownNewsActivityLift {
        event_timestamp_utc_nanos: i64,
        value: f64,
    },
    ObservedOpenSessionReturnOutsideSourceWindow {
        return_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    InvalidObservedOpenSessionReturn {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidObservedOpenSessionReturnElapsedTime {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidObservedOpenSessionActivityLift {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidObservedOpenSessionSellShareProxy {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    IntradayJumpOutsideSourceWindow {
        jump_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    InvalidIntradayJumpReturn {
        jump_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidIntradayActivityLift {
        jump_timestamp_utc_nanos: i64,
        value: f64,
    },
    CloseOpenBoundaryOutsideSourceWindow {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    NonIncreasingCloseOpenBoundaryWindow {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
    },
    InvalidCloseBoundaryPrice {
        close_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidOpenBoundaryPrice {
        open_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidCloseOpenBoundaryActivityLift {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidCloseOpenGapMaterialityTolerance {
        value: f64,
    },
    InvalidOpenSessionSellShareProxy {
        value: f64,
    },
    CloseOpenGapOutsideSourceWindow {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
        window_start_utc_nanos: i64,
        window_end_utc_nanos: i64,
    },
    NonIncreasingCloseOpenGapWindow {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
    },
    InvalidCloseOpenGapReturn {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidCloseOpenActivityLift {
        close_timestamp_utc_nanos: i64,
        open_timestamp_utc_nanos: i64,
        value: f64,
    },
    CloseOpenBoundaryStateResolutionFailed {
        exchange_code: String,
        timestamp_utc_nanos: i64,
        error_message: String,
    },
    InvalidCloseBoundaryTransition {
        exchange_code: String,
        close_timestamp_utc_nanos: i64,
        previous_state: MarketState,
        boundary_state: MarketState,
    },
    InvalidOpenBoundaryTransition {
        exchange_code: String,
        open_timestamp_utc_nanos: i64,
        previous_state: MarketState,
        boundary_state: MarketState,
    },
    InvalidSharedMarkLawTemplate {
        error_message: String,
    },
    ReturnMarkHourOfWeekFactorCountMismatch {
        expected_factor_count: usize,
        observed_factor_count: usize,
    },
    InvalidReturnMarkHourOfWeekFactor {
        factor_index: usize,
        value: f64,
    },
    InvalidReturnMarkProfileResolution {
        value: usize,
    },
    InvalidTickSize {
        value: f64,
    },
    InvalidContinuousDistributionFitWeight {
        value: f64,
    },
    MissingContinuousDistributionSeedCandidate,
    InvalidSurpriseArrivalFitWeight {
        value: f64,
    },
    InvalidOpenJumpFitWeight {
        value: f64,
    },
    InvalidClosedGapFitWeight {
        value: f64,
    },
    InvalidActivityCouplingFitWeight {
        value: f64,
    },
    InvalidPriceRangeSanityFitWeight {
        value: f64,
    },
}

impl Display for ReturnComponentsStageCandidateGenerationInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonIncreasingSourceWindow {
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "candidate generation source window must be increasing, found start={window_start_utc_nanos} and end={window_end_utc_nanos}"
            ),
            Self::InvalidKnownNewsCalendar { error_message } => write!(
                formatter,
                "known_news_calendar is invalid for candidate generation: {error_message}"
            ),
            Self::KnownNewsEventOutsideSourceWindow {
                timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "known-news calendar timestamp {timestamp_utc_nanos} is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::MissingObservedOpenSessionReturns => write!(
                formatter,
                "candidate generation requires at least one observed_open_session_return"
            ),
            Self::KnownNewsReturnSampleOutsideSourceWindow {
                event_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "known-news return sample timestamp {event_timestamp_utc_nanos} is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::KnownNewsAttributedOpenReturnOutsideSourceWindow {
                event_timestamp_utc_nanos,
                attributed_open_return_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "known-news sample at {event_timestamp_utc_nanos} attributes open return timestamp {attributed_open_return_timestamp_utc_nanos} outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::KnownNewsAttributedOpenReturnPrecedesEvent {
                event_timestamp_utc_nanos,
                attributed_open_return_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known-news sample at {event_timestamp_utc_nanos} attributes open return timestamp {attributed_open_return_timestamp_utc_nanos} before the event"
            ),
            Self::InvalidKnownNewsOpenJumpReturn {
                event_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "known-news open-window return sample at {event_timestamp_utc_nanos} must be finite, found {value}"
            ),
            Self::InvalidKnownNewsClosedGapReturn {
                event_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "known-news closed-gap return sample at {event_timestamp_utc_nanos} must be finite, found {value}"
            ),
            Self::InvalidKnownNewsActivityLift {
                event_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "known-news post-shock activity lift at {event_timestamp_utc_nanos} must be finite and non-negative, found {value}"
            ),
            Self::ObservedOpenSessionReturnOutsideSourceWindow {
                return_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "observed open-session return timestamp {return_timestamp_utc_nanos} is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::InvalidObservedOpenSessionReturn {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "observed open-session return at {return_timestamp_utc_nanos} must be finite, found {value}"
            ),
            Self::InvalidObservedOpenSessionReturnElapsedTime {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "observed open-session return elapsed time at {return_timestamp_utc_nanos} must be finite and positive, found {value}"
            ),
            Self::InvalidObservedOpenSessionActivityLift {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "observed open-session return activity lift at {return_timestamp_utc_nanos} must be finite and non-negative, found {value}"
            ),
            Self::InvalidObservedOpenSessionSellShareProxy {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "observed open-session return sell-share proxy at {return_timestamp_utc_nanos} must be finite and within [0, 1], found {value}"
            ),
            Self::IntradayJumpOutsideSourceWindow {
                jump_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "intraday jump timestamp {jump_timestamp_utc_nanos} is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::InvalidIntradayJumpReturn {
                jump_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "intraday jump return at {jump_timestamp_utc_nanos} must be finite, found {value}"
            ),
            Self::InvalidIntradayActivityLift {
                jump_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "intraday post-shock activity lift at {jump_timestamp_utc_nanos} must be finite and non-negative, found {value}"
            ),
            Self::CloseOpenBoundaryOutsideSourceWindow {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "close-open boundary window [{close_timestamp_utc_nanos}, {open_timestamp_utc_nanos}] is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::NonIncreasingCloseOpenBoundaryWindow {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
            } => write!(
                formatter,
                "close-open boundary window must be increasing, found close={close_timestamp_utc_nanos} and open={open_timestamp_utc_nanos}"
            ),
            Self::InvalidCloseBoundaryPrice {
                close_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "close boundary price at {close_timestamp_utc_nanos} must be finite and strictly positive, found {value}"
            ),
            Self::InvalidOpenBoundaryPrice {
                open_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "open boundary price at {open_timestamp_utc_nanos} must be finite and strictly positive, found {value}"
            ),
            Self::InvalidCloseOpenBoundaryActivityLift {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "close-open boundary post-shock activity lift for window [{close_timestamp_utc_nanos}, {open_timestamp_utc_nanos}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidCloseOpenGapMaterialityTolerance { value } => write!(
                formatter,
                "close_open_gap_detection_policy.material_gap_absolute_return_tolerance must be finite and non-negative, found {value}"
            ),
            Self::InvalidOpenSessionSellShareProxy { value } => write!(
                formatter,
                "open_session_sell_share_proxy must be finite and within [0, 1], found {value}"
            ),
            Self::CloseOpenGapOutsideSourceWindow {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
                window_start_utc_nanos,
                window_end_utc_nanos,
            } => write!(
                formatter,
                "close-open gap window [{close_timestamp_utc_nanos}, {open_timestamp_utc_nanos}] is outside source window [{window_start_utc_nanos}, {window_end_utc_nanos})"
            ),
            Self::NonIncreasingCloseOpenGapWindow {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
            } => write!(
                formatter,
                "close-open gap window must be increasing, found close={close_timestamp_utc_nanos} and open={open_timestamp_utc_nanos}"
            ),
            Self::InvalidCloseOpenGapReturn {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "close-open gap return for window [{close_timestamp_utc_nanos}, {open_timestamp_utc_nanos}] must be finite, found {value}"
            ),
            Self::InvalidCloseOpenActivityLift {
                close_timestamp_utc_nanos,
                open_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "close-open post-shock activity lift for window [{close_timestamp_utc_nanos}, {open_timestamp_utc_nanos}] must be finite and non-negative, found {value}"
            ),
            Self::CloseOpenBoundaryStateResolutionFailed {
                exchange_code,
                timestamp_utc_nanos,
                error_message,
            } => write!(
                formatter,
                "failed to resolve market state for exchange `{exchange_code}` at boundary timestamp {timestamp_utc_nanos}: {error_message}"
            ),
            Self::InvalidCloseBoundaryTransition {
                exchange_code,
                close_timestamp_utc_nanos,
                previous_state,
                boundary_state,
            } => write!(
                formatter,
                "close boundary for exchange `{exchange_code}` at {close_timestamp_utc_nanos} must transition from open to closed, found previous={previous_state:?} and boundary={boundary_state:?}"
            ),
            Self::InvalidOpenBoundaryTransition {
                exchange_code,
                open_timestamp_utc_nanos,
                previous_state,
                boundary_state,
            } => write!(
                formatter,
                "open boundary for exchange `{exchange_code}` at {open_timestamp_utc_nanos} must transition from closed to open, found previous={previous_state:?} and boundary={boundary_state:?}"
            ),
            Self::InvalidSharedMarkLawTemplate { error_message } => write!(
                formatter,
                "shared_mark_law_template is invalid for candidate generation: {error_message}"
            ),
            Self::ReturnMarkHourOfWeekFactorCountMismatch {
                expected_factor_count,
                observed_factor_count,
            } => write!(
                formatter,
                "return_mark_hour_of_week_scale_profile.hour_of_week_factors must contain {expected_factor_count} factors, found {observed_factor_count}"
            ),
            Self::InvalidReturnMarkHourOfWeekFactor {
                factor_index,
                value,
            } => write!(
                formatter,
                "return_mark_hour_of_week_scale_profile.hour_of_week_factors[{factor_index}] must be finite and strictly positive, found {value}"
            ),
            Self::InvalidReturnMarkProfileResolution { value } => write!(
                formatter,
                "return_mark_hour_of_week_scale_profile.profile_resolution_hours must be 1, found {value}"
            ),
            Self::InvalidTickSize { value } => write!(
                formatter,
                "tick_grid_observation_model.tick_size must be finite and strictly positive when provided, found {value}"
            ),
            Self::InvalidContinuousDistributionFitWeight { value } => write!(
                formatter,
                "continuous_distribution_fit_weight must be finite and non-negative, found {value}"
            ),
            Self::MissingContinuousDistributionSeedCandidate => write!(
                formatter,
                "continuous-return candidate generation produced no NIG seed candidates"
            ),
            Self::InvalidSurpriseArrivalFitWeight { value } => write!(
                formatter,
                "surprise_arrival_fit_weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidOpenJumpFitWeight { value } => write!(
                formatter,
                "open_jump_fit_weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidClosedGapFitWeight { value } => write!(
                formatter,
                "closed_gap_fit_weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidActivityCouplingFitWeight { value } => write!(
                formatter,
                "activity_coupling_fit_weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidPriceRangeSanityFitWeight { value } => write!(
                formatter,
                "price_range_sanity_fit_weight must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for ReturnComponentsStageCandidateGenerationInputError {}

/// Explicit known-news arrival control family generated from observed known-news samples.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsKnownNewsArrivalCandidateFamily {
    pub minimum_abs_known_return_threshold: f64,
    pub retained_known_event_timestamps_utc_nanos: Vec<i64>,
}

impl ReturnComponentsKnownNewsArrivalCandidateFamily {
    pub(super) fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        if !self.minimum_abs_known_return_threshold.is_finite()
            || self.minimum_abs_known_return_threshold < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidKnownArrivalThreshold {
                    value: self.minimum_abs_known_return_threshold,
                },
            );
        }

        let known_arrivals = KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: self
                .retained_known_event_timestamps_utc_nanos
                .clone(),
        };

        known_arrivals.validate().map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidKnownArrivalFamily {
                error_message: source.to_string(),
            }
        })?;

        Ok(())
    }
}

/// Explicit surprise-news arrival control family generated from detected jumps and gaps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    pub baseline_intensity: f64,
    pub weekly_hour_utc_multipliers: Vec<f64>,
    pub profile_resolution_hours: usize,
}

impl ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    pub(super) fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        SurpriseNewsArrivalConfiguration {
            baseline_intensity: self.baseline_intensity,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                error_message: source.to_string(),
            }
        })?;

        if self.profile_resolution_hours
            != super::constants::RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
        {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                    error_message: format!(
                        "surprise arrival weekly profile resolution must equal {} hour(s)",
                        super::constants::RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
                    ),
                },
            );
        }

        let expected_bucket_count =
            super::derived_contracts::return_components_stage_surprise_intensity_weekly_profile_bucket_count();
        if self.weekly_hour_utc_multipliers.len() != expected_bucket_count {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                    error_message: format!(
                        "surprise arrival weekly profile must contain exactly {expected_bucket_count} multipliers"
                    ),
                },
            );
        }

        if self
            .weekly_hour_utc_multipliers
            .iter()
            .any(|multiplier| !multiplier.is_finite() || *multiplier <= 0.0)
        {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                    error_message:
                        "surprise arrival weekly profile multipliers must be finite and strictly positive"
                            .to_string(),
                },
            );
        }

        Ok(())
    }
}

/// Explicit return-jump mark-scale candidate family generated from jump and gap bins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    pub sigma_g: f64,
}

impl ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        CoupledGapMarkConfiguration::new(self.sigma_g)
            .construct_gap_jump_mark(1.0, 1.0)
            .map_err(|source| {
                ReturnComponentsStageCandidateInputError::InvalidReturnJumpMarkScaleFamily {
                    error_message: source.to_string(),
                }
            })?;

        Ok(())
    }
}

/// Explicit news-to-Hawkes activity-coupling candidate family.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsActivityCouplingCandidateFamily {
    pub known_activity_coupling_scale: f64,
    pub surprise_activity_coupling_scale: f64,
}

impl ReturnComponentsActivityCouplingCandidateFamily {
    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: self.known_activity_coupling_scale,
            return_jump_scale: 1.0,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidActivityCouplingFamily {
                error_message: source.to_string(),
            }
        })?;

        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: self.surprise_activity_coupling_scale,
            return_jump_scale: 1.0,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidActivityCouplingFamily {
                error_message: source.to_string(),
            }
        })?;

        Ok(())
    }
}

/// Candidate-family grouping for deterministic return-components candidate generation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateFamilies {
    pub known_news_arrival_controls: ReturnComponentsKnownNewsArrivalCandidateFamily,
    pub surprise_news_arrival_controls: ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    pub return_jump_mark_scale_controls: ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    pub news_to_hawkes_activity_coupling_controls: ReturnComponentsActivityCouplingCandidateFamily,
}

impl ReturnComponentsStageCandidateFamilies {
    pub(super) fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        self.known_news_arrival_controls.validate()?;
        self.surprise_news_arrival_controls.validate()?;
        self.return_jump_mark_scale_controls.validate()?;
        self.news_to_hawkes_activity_coupling_controls.validate()?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReturnComponentsStageObservedSignalSummaryStatistics {
    pub known_calendar_event_count: usize,
    pub known_realized_sample_count: usize,
    pub source_window_duration_hours: f64,
    pub observed_open_session_return_count: usize,
    pub continuous_fit_return_count: usize,
    pub intraday_jump_candidate_count: usize,
    pub intraday_jump_episode_count: usize,
    pub selected_surprise_jump_episode_count: usize,
    pub close_open_gap_candidate_count: usize,
    pub close_open_gap_episode_count: usize,
    pub open_session_sell_share_proxy: f64,
    pub mean_continuous_business_time_scale: f64,
    pub continuous_return_mean: f64,
    pub continuous_return_volatility: f64,
    pub continuous_return_skewness: f64,
    pub continuous_return_excess_kurtosis: f64,
    pub observed_continuous_log_price_span: f64,
    pub observed_log_price_span: f64,
    pub mean_abs_known_open_jump_return: f64,
    pub mean_abs_known_closed_gap_return: f64,
    pub mean_abs_surprise_open_jump_return: f64,
    pub mean_abs_surprise_closed_gap_return: f64,
    pub mean_known_post_shock_activity_lift: f64,
    pub mean_surprise_post_shock_activity_lift: f64,
    pub known_activity_coupling_target: f64,
    pub surprise_activity_coupling_target: f64,
    pub known_news_coverage_ratio: f64,
    pub surprise_arrival_intensity_per_hour: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateSourceWindow {
    pub source_name: String,
    pub window_start_utc_nanos: i64,
    pub window_end_utc_nanos: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsStageDeterministicSeedMetadata {
    pub stage_seed: u64,
    pub candidate_generation_seed: u64,
    pub deterministic_seed_policy: &'static str,
}

/// Candidate provenance payload for deterministic artifact persistence and replay.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateProvenance {
    pub source_windows: Vec<ReturnComponentsStageCandidateSourceWindow>,
    pub preprocessing_filter_policy: &'static str,
    pub preprocessing_filters: Vec<String>,
    pub surprise_episode_selection_policy: ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    pub quantile_binning_policy: ReturnComponentsStageQuantileBinningPolicy,
    pub diagnostic_policy: ReturnComponentsStageDiagnosticPolicy,
    pub objective_scoring_policy: ReturnComponentsStageObjectiveScoringPolicy,
    pub price_range_sanity_policy: ReturnComponentsStagePriceRangeSanityPolicy,
    pub summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    pub deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata,
}

impl ReturnComponentsStageCandidateProvenance {
    pub(super) fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        if self.source_windows.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingCandidateSourceWindows);
        }
        if self.preprocessing_filters.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingPreprocessingFilters);
        }

        self.surprise_episode_selection_policy
            .validate()
            .map_err(|error_message| {
                ReturnComponentsStageCandidateInputError::InvalidSurpriseEpisodeSelectionPolicy {
                    error_message,
                }
            })?;

        self.quantile_binning_policy
            .validate()
            .map_err(|error_message| {
                ReturnComponentsStageCandidateInputError::InvalidQuantileBinningPolicy {
                    error_message,
                }
            })?;

        self.diagnostic_policy.validate().map_err(|error_message| {
            ReturnComponentsStageCandidateInputError::InvalidDiagnosticPolicy { error_message }
        })?;

        self.objective_scoring_policy
            .validate()
            .map_err(|error_message| {
                ReturnComponentsStageCandidateInputError::InvalidObjectiveScoringPolicy {
                    error_message,
                }
            })?;

        self.price_range_sanity_policy
            .validate()
            .map_err(|error_message| {
                ReturnComponentsStageCandidateInputError::InvalidPriceRangeSanityPolicy {
                    error_message,
                }
            })?;

        let summary_statistics = self.summary_statistics;

        for metric_value in [
            summary_statistics.open_session_sell_share_proxy,
            summary_statistics.continuous_return_volatility,
            summary_statistics.observed_log_price_span,
            summary_statistics.mean_abs_known_open_jump_return,
            summary_statistics.mean_abs_known_closed_gap_return,
            summary_statistics.mean_abs_surprise_open_jump_return,
            summary_statistics.mean_abs_surprise_closed_gap_return,
            summary_statistics.mean_known_post_shock_activity_lift,
            summary_statistics.mean_surprise_post_shock_activity_lift,
            summary_statistics.known_activity_coupling_target,
            summary_statistics.surprise_activity_coupling_target,
            summary_statistics.known_news_coverage_ratio,
            summary_statistics.surprise_arrival_intensity_per_hour,
        ] {
            if !metric_value.is_finite() || metric_value < 0.0 {
                return Err(
                    ReturnComponentsStageCandidateInputError::InvalidSummaryStatistic {
                        value: metric_value,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageDiagnosticPolicy {
    pub fit_policy: &'static str,
    pub insufficient_evidence_state: &'static str,
    pub identified_component_state: &'static str,
}

impl ReturnComponentsStageDiagnosticPolicy {
    fn validate(&self) -> Result<(), String> {
        if self.fit_policy.trim().is_empty() {
            return Err("fit_policy must be non-empty".to_string());
        }
        if self.insufficient_evidence_state.trim().is_empty() {
            return Err("insufficient_evidence_state must be non-empty".to_string());
        }
        if self.identified_component_state.trim().is_empty() {
            return Err("identified_component_state must be non-empty".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageObjectiveScoringPolicy {
    pub fit_policy: &'static str,
    pub arrival_frequency_zero_floor: f64,
    pub positive_metric_zero_floor: f64,
    pub price_range_threshold_floor: f64,
    pub price_range_excess_transform: &'static str,
    pub dominant_scale_driver_order: Vec<String>,
}

impl ReturnComponentsStageObjectiveScoringPolicy {
    fn validate(&self) -> Result<(), String> {
        if self.fit_policy.trim().is_empty() {
            return Err("fit_policy must be non-empty".to_string());
        }
        if self.price_range_excess_transform.trim().is_empty() {
            return Err("price_range_excess_transform must be non-empty".to_string());
        }
        if self.dominant_scale_driver_order.is_empty() {
            return Err("dominant_scale_driver_order must be non-empty".to_string());
        }

        for (field_name, value) in [
            (
                "arrival_frequency_zero_floor",
                self.arrival_frequency_zero_floor,
            ),
            (
                "positive_metric_zero_floor",
                self.positive_metric_zero_floor,
            ),
            (
                "price_range_threshold_floor",
                self.price_range_threshold_floor,
            ),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{field_name} must be finite and strictly positive"));
            }
        }

        if self
            .dominant_scale_driver_order
            .iter()
            .any(|value| !is_non_empty_string(value))
        {
            return Err("dominant_scale_driver_order entries must be non-empty".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageSurpriseEpisodeSelectionPolicy {
    pub fit_policy: &'static str,
    pub merge_gap_quantile: f64,
    pub merge_gap_threshold_nanos: i64,
    pub normalized_magnitude_quantile: f64,
    pub normalized_magnitude_threshold: f64,
    pub positive_activity_excess_quantile: f64,
    pub positive_activity_excess_threshold: f64,
    pub composite_score_quantile: f64,
    pub composite_score_threshold: f64,
}

impl ReturnComponentsStageSurpriseEpisodeSelectionPolicy {
    fn validate(&self) -> Result<(), String> {
        if self.fit_policy.trim().is_empty() {
            return Err("fit_policy must be non-empty".to_string());
        }

        for (field_name, value) in [
            ("merge_gap_quantile", self.merge_gap_quantile),
            (
                "normalized_magnitude_quantile",
                self.normalized_magnitude_quantile,
            ),
            (
                "positive_activity_excess_quantile",
                self.positive_activity_excess_quantile,
            ),
            ("composite_score_quantile", self.composite_score_quantile),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{field_name} must be finite and within [0, 1]"));
            }
        }

        if self.merge_gap_threshold_nanos < 0 {
            return Err("merge_gap_threshold_nanos must be non-negative".to_string());
        }

        for (field_name, value) in [
            (
                "normalized_magnitude_threshold",
                self.normalized_magnitude_threshold,
            ),
            (
                "positive_activity_excess_threshold",
                self.positive_activity_excess_threshold,
            ),
            ("composite_score_threshold", self.composite_score_threshold),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("{field_name} must be finite and non-negative"));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageQuantileBinningPolicy {
    pub fit_policy: &'static str,
    pub jump_bin_quantiles: Vec<f64>,
    pub gap_bin_quantiles: Vec<f64>,
    pub jump_abs_return_bin_edges: Vec<f64>,
    pub gap_abs_return_bin_edges: Vec<f64>,
}

impl ReturnComponentsStageQuantileBinningPolicy {
    fn validate(&self) -> Result<(), String> {
        if self.fit_policy.trim().is_empty() {
            return Err("fit_policy must be non-empty".to_string());
        }
        if self.jump_abs_return_bin_edges.is_empty() {
            return Err("jump_abs_return_bin_edges must be non-empty".to_string());
        }
        if self.gap_abs_return_bin_edges.is_empty() {
            return Err("gap_abs_return_bin_edges must be non-empty".to_string());
        }

        for (field_name, quantiles) in [
            ("jump_bin_quantiles", self.jump_bin_quantiles.as_slice()),
            ("gap_bin_quantiles", self.gap_bin_quantiles.as_slice()),
        ] {
            for &quantile in quantiles {
                if !quantile.is_finite() || !(0.0..=1.0).contains(&quantile) {
                    return Err(format!(
                        "{field_name} values must be finite and within [0, 1]"
                    ));
                }
            }
        }

        for (field_name, bin_edges) in [
            (
                "jump_abs_return_bin_edges",
                self.jump_abs_return_bin_edges.as_slice(),
            ),
            (
                "gap_abs_return_bin_edges",
                self.gap_abs_return_bin_edges.as_slice(),
            ),
        ] {
            for &bin_edge in bin_edges {
                if !bin_edge.is_finite() || bin_edge < 0.0 {
                    return Err(format!(
                        "{field_name} values must be finite and non-negative"
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStagePriceRangeSanityPolicy {
    pub fit_policy: &'static str,
    pub observed_continuous_log_price_span: f64,
    pub observed_component_reference_log_price_span: f64,
    pub maximum_representable_log_price_span: f64,
}

impl ReturnComponentsStagePriceRangeSanityPolicy {
    fn validate(&self) -> Result<(), String> {
        if self.fit_policy.trim().is_empty() {
            return Err("fit_policy must be non-empty".to_string());
        }

        for (field_name, value) in [
            (
                "observed_continuous_log_price_span",
                self.observed_continuous_log_price_span,
            ),
            (
                "observed_component_reference_log_price_span",
                self.observed_component_reference_log_price_span,
            ),
            (
                "maximum_representable_log_price_span",
                self.maximum_representable_log_price_span,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("{field_name} must be finite and non-negative"));
            }
        }

        if self.maximum_representable_log_price_span <= 0.0 {
            return Err(
                "maximum_representable_log_price_span must be finite and strictly positive"
                    .to_string(),
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsStageObjectiveMetricBuckets {
    pub continuous_distribution_fit: f64,
    pub surprise_arrival_fit: f64,
    pub open_jump_fit: f64,
    pub closed_gap_fit: f64,
    pub activity_coupling_fit: f64,
    pub price_range_sanity_fit: f64,
}

impl ReturnComponentsStageObjectiveMetricBuckets {
    pub(super) fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        for metric_value in [
            self.continuous_distribution_fit,
            self.surprise_arrival_fit,
            self.open_jump_fit,
            self.closed_gap_fit,
            self.activity_coupling_fit,
            self.price_range_sanity_fit,
        ] {
            if !metric_value.is_finite() || metric_value < 0.0 {
                return Err(
                    ReturnComponentsStageCandidateInputError::InvalidMetricBucketValue {
                        value: metric_value,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsStageReturnDistributionDiagnostic {
    pub sample_count: usize,
    pub observed_mean_return: f64,
    pub fitted_mean_return: f64,
    pub observed_return_volatility: f64,
    pub fitted_return_volatility: f64,
    pub observed_skewness: f64,
    pub fitted_skewness: f64,
    pub observed_excess_kurtosis: f64,
    pub fitted_excess_kurtosis: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageJumpDiagnostic {
    pub evidence_state: String,
    pub observed_jump_count: usize,
    pub observed_mean_abs_jump_return: f64,
    pub fitted_mean_abs_jump_return: f64,
    pub fitted_sigma_g: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageSurpriseArrivalDiagnostic {
    pub evidence_state: String,
    pub observed_jump_count: usize,
    pub observed_jump_frequency_per_hour: f64,
    pub fitted_surprise_arrival_intensity_per_hour: f64,
    pub surprise_arrival_profile_fit_error: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageGapDiagnostic {
    pub evidence_state: String,
    pub observed_gap_count: usize,
    pub observed_gap_frequency_per_hour: f64,
    pub observed_mean_abs_gap_return: f64,
    pub fitted_mean_abs_gap_return: f64,
    pub fitted_sigma_g: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageActivityCouplingDiagnostic {
    pub evidence_state: String,
    pub observed_known_post_shock_activity_lift: f64,
    pub observed_surprise_post_shock_activity_lift: f64,
    pub fitted_known_activity_coupling_scale: f64,
    pub fitted_surprise_activity_coupling_scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStagePriceRangeSanityDiagnostic {
    pub policy: ReturnComponentsStagePriceRangeSanityPolicy,
    pub is_flagged: bool,
    pub observed_log_price_span: f64,
    pub observed_component_reference_log_price_span: f64,
    pub admissible_reference_log_price_span: f64,
    pub fitted_reference_log_price_span: f64,
    pub continuous_reference_log_price_span: f64,
    pub effective_continuous_sample_count: usize,
    pub effective_mean_business_time_scale: f64,
    pub fitted_mean_abs_jump_return: f64,
    pub expansion_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageContinuousReturnSampleDiagnostic {
    pub observed_returns: Vec<f64>,
    pub fitted_returns: Vec<f64>,
    pub business_time_scales: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
    pub family_label: String,
    pub seed_objective: f64,
    pub local_objective: f64,
    pub final_objective: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: String,
    pub seed_log_alpha: f64,
    pub seed_buy_skew_coordinate: f64,
    pub seed_sell_relative_coordinate: f64,
    pub seed_log_delta_offset: f64,
    pub final_log_alpha: f64,
    pub final_buy_skew_coordinate: f64,
    pub final_sell_relative_coordinate: f64,
    pub final_log_delta_offset: f64,
    pub final_alpha: f64,
    pub final_beta_0: f64,
    pub final_delta: f64,
    pub final_delta_beta: f64,
    pub profile_fit_score: f64,
    pub fitted_return_volatility: f64,
    pub fitted_skewness: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReturnComponentsStageExactLikelihoodFailureScope {
    Global,
    ConditionalScaleBucket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReturnComponentsStageExactLikelihoodFailureReason {
    NoEvaluatedBlocks,
    InvalidObservedInputs,
    InvalidBlockStandardDeviation,
    InvalidStandardizedDecayScale,
    InvalidInitialIntegrationUpperBound,
    IntegrationStepCountOverflow,
    InvalidCharacteristicExponent,
    InvalidCharacteristicFunction,
    InvalidIntegrand,
    InvalidDensity,
    LogDensityErrorNotCertified,
    IntegrationBoundOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReturnComponentsStageExactLikelihoodEvaluatorKind {
    ClosedFormSingleSideBuy,
    ClosedFormSingleSideSell,
    TiltedMixedInversion,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExactLikelihoodFailureDiagnostic {
    pub scope: ReturnComponentsStageExactLikelihoodFailureScope,
    pub scale_bucket_index: Option<usize>,
    pub block_member_index: Option<usize>,
    pub failure_reason: ReturnComponentsStageExactLikelihoodFailureReason,
    pub evaluator_kind: Option<ReturnComponentsStageExactLikelihoodEvaluatorKind>,
    pub observed_return: Option<f64>,
    pub business_time_scale: Option<f64>,
    pub sell_share_proxy: Option<f64>,
    pub integrated_buy_intensity: Option<f64>,
    pub integrated_sell_intensity: Option<f64>,
    pub block_standard_deviation: Option<f64>,
    pub standardized_return: Option<f64>,
    pub tilt_theta: Option<f64>,
    pub tilt_domain_lower_theta_exclusive: Option<f64>,
    pub tilt_domain_upper_theta_exclusive: Option<f64>,
    pub standardized_decay_scale: Option<f64>,
    pub integration_upper_bound: Option<f64>,
    pub log_density_resolution: Option<f64>,
    pub scope_average_log_likelihood_error_upper_bound: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    pub global_fit_score: f64,
    pub conditional_fit_score: f64,
    pub boundary_penalty: f64,
    pub profile_fit_score: f64,
    pub log_likelihood_shift: f64,
    pub optimizer_seed_objective: f64,
    pub optimizer_local_objective: f64,
    pub optimizer_final_objective: f64,
    pub optimizer_local_evaluation_count: usize,
    pub optimizer_stop_reason: String,
    pub proxy_em_best_objective: Option<f64>,
    pub proxy_em_final_convergence_ratio: Option<f64>,
    pub proxy_em_evaluation_count: usize,
    pub proxy_em_completed_seed_count: usize,
    pub proxy_em_refined_seed_count: usize,
    pub proxy_em_requested_seed_count: usize,
    pub proxy_em_iteration_budget_per_seed: usize,
    pub proxy_em_fitting_sample_count: usize,
    pub proxy_em_fitting_sample_weight_sum: f64,
    pub proxy_em_converged_seed_count: usize,
    pub proxy_em_non_improving_seed_count: usize,
    pub proxy_em_numerical_failure_seed_count: usize,
    pub proxy_em_iteration_budget_exhausted_seed_count: usize,
    pub proxy_em_projection_failure_seed_count: usize,
    pub proxy_em_invalid_initial_seed_count: usize,
    pub optimizer_basin_diagnostics: Vec<ReturnComponentsStageContinuousOptimizerBasinDiagnostic>,
    pub exact_likelihood_failures: Vec<ReturnComponentsStageExactLikelihoodFailureDiagnostic>,
    pub exact_closed_form_single_side_buy_block_count: usize,
    pub exact_closed_form_single_side_sell_block_count: usize,
    pub exact_tilted_mixed_inversion_block_count: usize,
    pub exact_structural_failure_count: usize,
    pub exact_uncertified_scope_count: usize,
    pub exact_global_average_log_likelihood_error_upper_bound: Option<f64>,
    pub exact_conditional_average_log_likelihood_error_upper_bound: Option<f64>,
    pub mean_return_fit: f64,
    pub volatility_fit: f64,
    pub median_return_fit: f64,
    pub lower_quartile_fit: f64,
    pub upper_quartile_fit: f64,
    pub lower_tail_q05_fit: f64,
    pub upper_tail_q95_fit: f64,
    pub extreme_left_tail_q01_fit: f64,
    pub extreme_right_tail_q99_fit: f64,
    pub mean_abs_return_fit: f64,
    pub median_abs_return_fit: f64,
    pub abs_quantile_ratio_fit: f64,
    pub bowley_skewness_fit: f64,
    pub l_moment_lambda2_fit: f64,
    pub l_moment_tau3_fit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateFamilyCounts {
    pub nig_candidate_count: usize,
    pub known_news_arrival_family_count: usize,
    pub surprise_news_arrival_family_count: usize,
    pub return_jump_mark_scale_family_count: usize,
    pub activity_coupling_family_count: usize,
}

/// Candidate parameterization and objective inputs for return-components-stage fitting.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageCandidateInput {
    pub candidate_id: String,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
    pub objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
    pub return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    pub continuous_distribution_fit_breakdown:
        ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    pub surprise_arrival_diagnostic: ReturnComponentsStageSurpriseArrivalDiagnostic,
    pub jump_diagnostic: ReturnComponentsStageJumpDiagnostic,
    pub gap_diagnostic: ReturnComponentsStageGapDiagnostic,
    pub activity_coupling_diagnostic: ReturnComponentsStageActivityCouplingDiagnostic,
    pub price_range_sanity_diagnostic: ReturnComponentsStagePriceRangeSanityDiagnostic,
    pub dominant_scale_driver: String,
    pub objective_terms: Vec<ReturnComponentsStageObjectiveTermInput>,
    pub candidate_families: ReturnComponentsStageCandidateFamilies,
    pub candidate_provenance: ReturnComponentsStageCandidateProvenance,
    pub return_mark_hour_of_week_scale_profile: ReturnMarkHourOfWeekScaleProfile,
    pub tick_grid_observation_model: TickGridObservationModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageCandidateInputError {
    MissingCandidateId,
    MissingObjectiveTerms,
    MissingObjectiveMetricBucketTerm {
        term_id: &'static str,
    },
    InvalidKnownArrivalThreshold {
        value: f64,
    },
    InvalidKnownArrivalFamily {
        error_message: String,
    },
    InvalidSurpriseArrivalFamily {
        error_message: String,
    },
    InvalidReturnJumpMarkScaleFamily {
        error_message: String,
    },
    InvalidActivityCouplingFamily {
        error_message: String,
    },
    MissingCandidateSourceWindows,
    MissingPreprocessingFilters,
    InvalidSurpriseEpisodeSelectionPolicy {
        error_message: String,
    },
    InvalidQuantileBinningPolicy {
        error_message: String,
    },
    InvalidDiagnosticPolicy {
        error_message: String,
    },
    InvalidObjectiveScoringPolicy {
        error_message: String,
    },
    InvalidPriceRangeSanityPolicy {
        error_message: String,
    },
    InvalidSummaryStatistic {
        value: f64,
    },
    InvalidMetricBucketValue {
        value: f64,
    },
    InvalidObjectiveTerm {
        term_id: String,
        source: HawkesStageObjectiveTermInputError,
    },
    InvalidWeightedObjectiveValue {
        value: f64,
    },
}

impl Display for ReturnComponentsStageCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => write!(formatter, "candidate_id must be non-empty"),
            Self::MissingObjectiveTerms => {
                write!(
                    formatter,
                    "candidate objective_terms must include at least one weighted term"
                )
            }
            Self::MissingObjectiveMetricBucketTerm { term_id } => write!(
                formatter,
                "objective terms must include required metric-bucket term `{term_id}`"
            ),
            Self::InvalidKnownArrivalThreshold { value } => write!(
                formatter,
                "known-news arrival threshold must be finite and non-negative, found {value}"
            ),
            Self::InvalidKnownArrivalFamily { error_message } => write!(
                formatter,
                "known-news arrival family is invalid: {error_message}"
            ),
            Self::InvalidSurpriseArrivalFamily { error_message } => write!(
                formatter,
                "surprise-news arrival family is invalid: {error_message}"
            ),
            Self::InvalidReturnJumpMarkScaleFamily { error_message } => write!(
                formatter,
                "return-jump mark-scale family is invalid: {error_message}"
            ),
            Self::InvalidActivityCouplingFamily { error_message } => write!(
                formatter,
                "activity-coupling family is invalid: {error_message}"
            ),
            Self::MissingCandidateSourceWindows => write!(
                formatter,
                "candidate provenance must include at least one source window"
            ),
            Self::MissingPreprocessingFilters => write!(
                formatter,
                "candidate provenance must include at least one preprocessing filter"
            ),
            Self::InvalidSurpriseEpisodeSelectionPolicy { error_message } => write!(
                formatter,
                "candidate provenance surprise-episode selection policy is invalid: {error_message}"
            ),
            Self::InvalidQuantileBinningPolicy { error_message } => write!(
                formatter,
                "candidate provenance quantile-binning policy is invalid: {error_message}"
            ),
            Self::InvalidDiagnosticPolicy { error_message } => write!(
                formatter,
                "candidate provenance diagnostic policy is invalid: {error_message}"
            ),
            Self::InvalidObjectiveScoringPolicy { error_message } => write!(
                formatter,
                "candidate provenance objective-scoring policy is invalid: {error_message}"
            ),
            Self::InvalidPriceRangeSanityPolicy { error_message } => write!(
                formatter,
                "candidate provenance price-range sanity policy is invalid: {error_message}"
            ),
            Self::InvalidSummaryStatistic { value } => write!(
                formatter,
                "candidate summary statistics values must be finite and non-negative, found {value}"
            ),
            Self::InvalidMetricBucketValue { value } => write!(
                formatter,
                "objective metric bucket values must be finite and non-negative, found {value}"
            ),
            Self::InvalidObjectiveTerm { term_id, source } => {
                write!(formatter, "objective term `{term_id}` is invalid: {source}")
            }
            Self::InvalidWeightedObjectiveValue { value } => write!(
                formatter,
                "weighted objective sum must be finite, found {value}"
            ),
        }
    }
}

impl Error for ReturnComponentsStageCandidateInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidObjectiveTerm { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl ReturnComponentsStageCandidateInput {
    pub(super) fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingCandidateId);
        }
        if self.objective_terms.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingObjectiveTerms);
        }

        self.candidate_families.validate()?;
        self.candidate_provenance.validate()?;
        self.objective_metric_buckets.validate()?;

        let mut has_continuous_distribution_term = false;
        let mut has_surprise_arrival_term = false;
        let mut has_open_jump_term = false;
        let mut has_closed_gap_term = false;
        let mut has_activity_coupling_term = false;
        let mut has_price_range_sanity_term = false;

        for objective_term in &self.objective_terms {
            objective_term.validate().map_err(|source| {
                ReturnComponentsStageCandidateInputError::InvalidObjectiveTerm {
                    term_id: objective_term.term_id.clone(),
                    source,
                }
            })?;

            if objective_term.term_id
                == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT
            {
                has_continuous_distribution_term = true;
            }

            if objective_term.term_id == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT
            {
                has_surprise_arrival_term = true;
            }

            if objective_term.term_id == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT {
                has_open_jump_term = true;
            }

            if objective_term.term_id == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT {
                has_closed_gap_term = true;
            }

            if objective_term.term_id
                == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT
            {
                has_activity_coupling_term = true;
            }

            if objective_term.term_id
                == RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT
            {
                has_price_range_sanity_term = true;
            }
        }

        if !has_continuous_distribution_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT,
                },
            );
        }

        if !has_open_jump_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT,
                },
            );
        }

        if !has_surprise_arrival_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT,
                },
            );
        }

        if !has_closed_gap_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT,
                },
            );
        }

        if !has_activity_coupling_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT,
                },
            );
        }

        if !has_price_range_sanity_term {
            return Err(
                ReturnComponentsStageCandidateInputError::MissingObjectiveMetricBucketTerm {
                    term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT,
                },
            );
        }

        Ok(())
    }
}

/// Upstream activity-side handoff input resolved for return-components-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageActivityInput {
    InMemoryOutput {
        payload: HawkesStageFittedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: Option<HawkesStageFittedPayload>,
    },
}

impl ReturnComponentsStageActivityInput {
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

    pub(super) fn selected_hawkes_candidate_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.selected_candidate_id.as_str()),
            Self::PersistedArtifactReference { payload, .. } => payload
                .as_ref()
                .map(|payload| payload.selected_candidate_id.as_str()),
        }
    }

    pub(super) fn hawkes_payload(&self) -> Option<&HawkesStageFittedPayload> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload),
            Self::PersistedArtifactReference { payload, .. } => payload.as_ref(),
        }
    }
}

/// Input payload for standalone or pipeline-integrated return-components-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageExecutionInput {
    pub activity_input: ReturnComponentsStageActivityInput,
    pub candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
}

impl ReturnComponentsStageExecutionInput {
    pub fn from_in_memory_activity_output(
        hawkes_payload: HawkesStageFittedPayload,
        candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    ) -> Self {
        Self {
            activity_input: ReturnComponentsStageActivityInput::InMemoryOutput {
                payload: hawkes_payload,
            },
            candidate_generation_input,
        }
    }

    pub fn from_persisted_activity_artifact_reference(
        artifact_reference: impl Into<String>,
        candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    ) -> Self {
        Self::from_persisted_activity_artifact_reference_with_payload(
            artifact_reference,
            None,
            candidate_generation_input,
        )
    }

    pub fn from_persisted_activity_artifact_reference_with_payload(
        artifact_reference: impl Into<String>,
        hawkes_payload: Option<HawkesStageFittedPayload>,
        candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    ) -> Self {
        Self {
            activity_input: ReturnComponentsStageActivityInput::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
                payload: hawkes_payload,
            },
            candidate_generation_input,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageConstraintViolationDiagnostic {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReturnComponentsStageActivityCalibrationContextSnapshot {
    pub modeled_open_event_rate_per_hour: f64,
    pub representative_interarrival_hours: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExactCandidateEvaluation {
    pub raw_objective_value: f64,
    pub objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
    pub return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    pub continuous_return_sample_diagnostic: ReturnComponentsStageContinuousReturnSampleDiagnostic,
    pub continuous_distribution_fit_breakdown:
        ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    pub surprise_arrival_diagnostic: ReturnComponentsStageSurpriseArrivalDiagnostic,
    pub jump_diagnostic: ReturnComponentsStageJumpDiagnostic,
    pub gap_diagnostic: ReturnComponentsStageGapDiagnostic,
    pub activity_coupling_diagnostic: ReturnComponentsStageActivityCouplingDiagnostic,
    pub price_range_sanity_diagnostic: ReturnComponentsStagePriceRangeSanityDiagnostic,
    pub constraint_violation_count: usize,
    pub constraint_violations: Vec<ReturnComponentsStageConstraintViolationDiagnostic>,
    pub dominant_scale_driver: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageExactCandidateEvaluationError {
    InvalidCandidateGenerationInput { error_message: String },
    InvalidFittedParameters { error_message: String },
}

impl Display for ReturnComponentsStageExactCandidateEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCandidateGenerationInput { error_message } => {
                write!(
                    formatter,
                    "invalid return-components candidate generation input: {error_message}"
                )
            }
            Self::InvalidFittedParameters { error_message } => {
                write!(
                    formatter,
                    "invalid return-components fitted parameters: {error_message}"
                )
            }
        }
    }
}

impl Error for ReturnComponentsStageExactCandidateEvaluationError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateDiagnostic {
    pub candidate_id: String,
    pub raw_objective_value: f64,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub constraint_violation_count: usize,
    pub constraint_violations: Vec<ReturnComponentsStageConstraintViolationDiagnostic>,
    pub objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
    pub objective_terms: Vec<ReturnComponentsStageObjectiveTermDiagnostic>,
    pub candidate_families: ReturnComponentsStageCandidateFamilies,
    pub candidate_provenance: ReturnComponentsStageCandidateProvenance,
    pub return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    pub continuous_distribution_fit_breakdown:
        ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    pub surprise_arrival_diagnostic: ReturnComponentsStageSurpriseArrivalDiagnostic,
    pub jump_diagnostic: ReturnComponentsStageJumpDiagnostic,
    pub gap_diagnostic: ReturnComponentsStageGapDiagnostic,
    pub activity_coupling_diagnostic: ReturnComponentsStageActivityCouplingDiagnostic,
    pub price_range_sanity_diagnostic: ReturnComponentsStagePriceRangeSanityDiagnostic,
    pub dominant_scale_driver: String,
}

/// Stable return-components-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic>,
    pub candidate_generation_policy: &'static str,
    pub objective_weighting_policy: &'static str,
    pub objective_metric_bucket_policy: &'static str,
    pub candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
    pub activity_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_selected_hawkes_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_artifact_reference: Option<String>,
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub candidate_generation_seed: u64,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
    pub objective_weighting_policy: &'static str,
    pub objective_metric_bucket_policy: &'static str,
    pub candidate_generation_policy: &'static str,
    pub candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
}

/// Structured failure modes for return-components-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum ReturnComponentsStageExecutionFailure {
    InvalidRunSpec {
        error_message: String,
    },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    ActivityInputTransportMismatch {
        expected_transport: CalibrationStageHandoffTransport,
        provided_transport: CalibrationStageHandoffTransport,
    },
    MissingArtifactReferenceInActivityInput,
    InvalidArtifactReferenceInActivityInput {
        artifact_reference: String,
    },
    ActivityArtifactReferenceMismatch {
        expected_artifact_reference: String,
        provided_artifact_reference: String,
    },
    InvalidCandidateGenerationInput {
        error_message: String,
    },
    CandidateGenerationFailed {
        error_message: String,
    },
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        error_message: String,
    },
    ConstraintViolationsForAllCandidates {
        candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic>,
    },
    CandidateConstructionFailed {
        candidate_id: String,
        error_message: String,
    },
}

/// Structured return-components-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<ReturnComponentsStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReturnComponentsStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReturnComponentsStageExecutionFailure>,
}
