use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::calendar::MarketState;
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
    ReturnComponentsStageFittedParameters,
};
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
        duration_nanos as f64 / 3_600_000_000_000.0
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

        self.objective_metric_weights.validate()?;

        Ok(())
    }
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
    InvalidContinuousDistributionFitWeight {
        value: f64,
    },
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
            Self::InvalidContinuousDistributionFitWeight { value } => write!(
                formatter,
                "continuous_distribution_fit_weight must be finite and non-negative, found {value}"
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

        if self.profile_resolution_hours != 1 {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                    error_message: "surprise arrival weekly profile resolution must equal one hour"
                        .to_string(),
                },
            );
        }
        if self.weekly_hour_utc_multipliers.len() != 168 {
            return Err(
                ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                    error_message:
                        "surprise arrival weekly profile must contain exactly 168 hourly multipliers"
                            .to_string(),
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
    pub jump_abs_return_bin_edges: Vec<f64>,
    pub gap_abs_return_bin_edges: Vec<f64>,
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
        if self.jump_abs_return_bin_edges.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingJumpBinEdges);
        }
        if self.gap_abs_return_bin_edges.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingGapBinEdges);
        }

        for &bin_edge in &self.jump_abs_return_bin_edges {
            if !bin_edge.is_finite() || bin_edge < 0.0 {
                return Err(
                    ReturnComponentsStageCandidateInputError::InvalidJumpBinEdge {
                        value: bin_edge,
                    },
                );
            }
        }
        for &bin_edge in &self.gap_abs_return_bin_edges {
            if !bin_edge.is_finite() || bin_edge < 0.0 {
                return Err(
                    ReturnComponentsStageCandidateInputError::InvalidGapBinEdge { value: bin_edge },
                );
            }
        }

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
    pub is_flagged: bool,
    pub observed_log_price_span: f64,
    pub fitted_reference_log_price_span: f64,
    pub continuous_reference_log_price_span: f64,
    pub effective_continuous_sample_count: usize,
    pub effective_mean_business_time_scale: f64,
    pub fitted_mean_abs_jump_return: f64,
    pub expansion_ratio: f64,
    pub threshold_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageContinuousReturnSampleDiagnostic {
    pub observed_returns: Vec<f64>,
    pub fitted_returns: Vec<f64>,
    pub business_time_scales: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    pub global_fit_score: f64,
    pub conditional_fit_score: f64,
    pub boundary_penalty: f64,
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
    pub surprise_arrival_diagnostic: ReturnComponentsStageSurpriseArrivalDiagnostic,
    pub jump_diagnostic: ReturnComponentsStageJumpDiagnostic,
    pub gap_diagnostic: ReturnComponentsStageGapDiagnostic,
    pub activity_coupling_diagnostic: ReturnComponentsStageActivityCouplingDiagnostic,
    pub price_range_sanity_diagnostic: ReturnComponentsStagePriceRangeSanityDiagnostic,
    pub dominant_scale_driver: String,
    pub objective_terms: Vec<ReturnComponentsStageObjectiveTermInput>,
    pub candidate_families: ReturnComponentsStageCandidateFamilies,
    pub candidate_provenance: ReturnComponentsStageCandidateProvenance,
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
    MissingJumpBinEdges,
    MissingGapBinEdges,
    InvalidJumpBinEdge {
        value: f64,
    },
    InvalidGapBinEdge {
        value: f64,
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
            Self::MissingJumpBinEdges => write!(
                formatter,
                "candidate provenance must include jump_abs_return_bin_edges"
            ),
            Self::MissingGapBinEdges => write!(
                formatter,
                "candidate provenance must include gap_abs_return_bin_edges"
            ),
            Self::InvalidJumpBinEdge { value } => write!(
                formatter,
                "jump_abs_return_bin_edges values must be finite and non-negative, found {value}"
            ),
            Self::InvalidGapBinEdge { value } => write!(
                formatter,
                "gap_abs_return_bin_edges values must be finite and non-negative, found {value}"
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
