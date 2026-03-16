use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::super::objective::{
    CalibrationObjectiveTermDiagnostic, HawkesStageObjectiveTermInput,
    HawkesStageObjectiveTermInputError,
};
use super::super::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunMetadata,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use super::hawkes::HawkesStageFittedPayload;
use super::seasonality::CalibrationStageExecutionStatus;
use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    ReturnComponentsStageFittedParameters, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

const RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY: &str =
    "observed_open_session_returns_and_shock_evidence_to_nig_search_and_news_candidate_families.v1";
const RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY: &str = "timestamp_sorted_finite_signal_filtering_with_repeated_shock_and_continuous_return_partitioning.v1";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY: &str =
    "continuous_distribution_jump_gap_activity_and_price_sanity_bucketed_weighted_terms.v1";
const RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY: &str =
    "stage_seed_reused_for_candidate_generation_enumeration.v1";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT: &str =
    "continuous_distribution_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT: &str = "open_jump_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT: &str = "closed_gap_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT: &str = "activity_coupling_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT: &str =
    "price_range_sanity_fit";
const RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_KNOWN_RETENTION_QUANTILES: [f64; 2] = [0.0, 0.5];
const RETURN_COMPONENTS_STAGE_KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES: i64 = 15;
const RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT: usize = 2;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER: f64 = 4.0;
const RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO: f64 = 4.0;
const RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER: f64 = 1.0e-6;
const RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES: usize = 16;
const RETURN_COMPONENTS_STAGE_MAX_RETURN_JUMP_MARK_SCALE_FAMILIES: usize = 9;
const RETURN_COMPONENTS_STAGE_MAX_ACTIVITY_COUPLING_FAMILIES: usize = 6;

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
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
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

    fn duration_hours(self) -> f64 {
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
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
    fn validate(
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
}

impl ReturnComponentsObservedOpenSessionReturn {
    fn validate(
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
    fn validate(
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
    fn validate(
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
    fn validate(
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

    fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
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
    pub jump_fit_weight: f64,
    pub gap_fit_weight: f64,
    pub activity_coupling_fit_weight: f64,
    pub price_range_sanity_fit_weight: f64,
}

impl ReturnComponentsStageObjectiveMetricWeights {
    pub const fn new(
        continuous_distribution_fit_weight: f64,
        jump_fit_weight: f64,
        gap_fit_weight: f64,
        activity_coupling_fit_weight: f64,
        price_range_sanity_fit_weight: f64,
    ) -> Self {
        Self {
            continuous_distribution_fit_weight,
            jump_fit_weight,
            gap_fit_weight,
            activity_coupling_fit_weight,
            price_range_sanity_fit_weight,
        }
    }

    fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if !self.continuous_distribution_fit_weight.is_finite()
            || self.continuous_distribution_fit_weight < 0.0
        {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidContinuousDistributionFitWeight {
                    value: self.continuous_distribution_fit_weight,
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    pub baseline_intensity: f64,
}

impl ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        SurpriseNewsArrivalConfiguration {
            baseline_intensity: self.baseline_intensity,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidSurpriseArrivalFamily {
                error_message: source.to_string(),
            }
        })
    }
}

/// Explicit return-jump mark-scale candidate family generated from jump and gap bins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    pub sigma_g: f64,
}

impl ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
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
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        self.known_news_arrival_controls.validate()?;
        self.surprise_news_arrival_controls.validate()?;
        self.return_jump_mark_scale_controls.validate()?;
        self.news_to_hawkes_activity_coupling_controls.validate()?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnComponentsStageObservedSignalSummaryStatistics {
    pub known_calendar_event_count: usize,
    pub known_realized_sample_count: usize,
    pub source_window_duration_hours: f64,
    pub observed_open_session_return_count: usize,
    pub continuous_fit_return_count: usize,
    pub intraday_jump_candidate_count: usize,
    pub close_open_gap_candidate_count: usize,
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
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
    pub open_jump_fit: f64,
    pub closed_gap_fit: f64,
    pub activity_coupling_fit: f64,
    pub price_range_sanity_fit: f64,
}

impl ReturnComponentsStageObjectiveMetricBuckets {
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        for metric_value in [
            self.continuous_distribution_fit,
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
    pub observed_jump_frequency_per_hour: f64,
    pub observed_mean_abs_jump_return: f64,
    pub fitted_mean_abs_jump_return: f64,
    pub fitted_sigma_g: f64,
    pub fitted_surprise_arrival_intensity_per_hour: f64,
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
    pub expansion_ratio: f64,
    pub threshold_ratio: f64,
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
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
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

    fn selected_hawkes_candidate_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.selected_candidate_id.as_str()),
            Self::PersistedArtifactReference { payload, .. } => payload
                .as_ref()
                .map(|payload| payload.selected_candidate_id.as_str()),
        }
    }

    fn hawkes_payload(&self) -> Option<&HawkesStageFittedPayload> {
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

struct ReturnComponentsStageCandidateGenerationOutput {
    candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
}

/// Execute the return-components stage with NIG and coupled-gap parameter constraints.
pub fn run_return_components_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: ReturnComponentsStageExecutionInput,
) -> ReturnComponentsStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::ReturnComponents)
    {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnComponents)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if let Err(source) = stage_input.candidate_generation_input.validate() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: source.to_string(),
            },
        );
    }

    let prepared_candidate_generation_input = match prepare_candidate_generation_input(
        &run_spec.data_inputs.exchange_code,
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
    ) {
        Ok(prepared_candidate_generation_input) => prepared_candidate_generation_input,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let expected_activity_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Hawkes
            && stage_handoff.to_stage == CalibrationStage::ReturnComponents
    });
    let expected_transport = expected_activity_handoff
        .map(|stage_handoff| stage_handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.activity_input.transport();

    if provided_transport != expected_transport {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::MissingArtifactReferenceInActivityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidArtifactReferenceInActivityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_activity_handoff
            && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
            && expected_artifact_reference != provided_artifact_reference
        {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::ActivityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.to_string(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::ReturnComponents);
    let stage_seed = resolved_stage_seed.stage_seed;
    let candidate_generation_seed = stage_seed;

    let generated_candidates = match generate_return_components_stage_candidate_inputs(
        &prepared_candidate_generation_input.candidate_generation_input,
        prepared_candidate_generation_input
            .derived_close_open_gap_candidates
            .as_slice(),
        prepared_candidate_generation_input
            .close_open_gap_detection_filter
            .as_str(),
        resolve_activity_calibration_context(&stage_input.activity_input),
        candidate_generation_seed,
    ) {
        Ok(output) => output,

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    if generated_candidates.candidate_inputs.is_empty() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                error_message: "candidate generation completed but produced zero candidate_inputs"
                    .to_string(),
            },
        );
    }

    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
        stage_solver_settings.clone(),
        return_components_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut seen_candidate_ids = BTreeSet::new();
    let mut external_constraint_diagnostics =
        Vec::<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>::with_capacity(
            generated_candidates.candidate_inputs.len(),
        );
    let mut adapter_candidate_inputs =
        Vec::with_capacity(generated_candidates.candidate_inputs.len());
    for candidate_input in &generated_candidates.candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }

        let constraint_violations =
            collect_return_components_stage_constraint_violations(candidate_input);
        let external_constraint_violation_count = constraint_violations.len();
        external_constraint_diagnostics.push(constraint_violations);

        adapter_candidate_inputs.push(CalibrationOptimizerAdapterCandidateInput {
            candidate_id: candidate_input.candidate_id.clone(),
            objective_terms: candidate_input.objective_terms.clone(),
            parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
                value: candidate_input
                    .nig_shared_plus_shift_configuration
                    .delta_beta,
            }],
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
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId { candidate_id },
            );
        }

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic> =
        Vec::with_capacity(generated_candidates.candidate_inputs.len());
    for ((candidate_input, constraint_violations), optimizer_diagnostic) in generated_candidates
        .candidate_inputs
        .iter()
        .zip(external_constraint_diagnostics.into_iter())
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
    {
        let mut combined_constraint_violations = constraint_violations;
        for bound_violation in &optimizer_diagnostic.bound_violations {
            combined_constraint_violations.push(
                ReturnComponentsStageConstraintViolationDiagnostic {
                    field: bound_violation.parameter_name.clone(),
                    message: format!(
                        "value {} violated inclusive bounds [{}, {}] ({:?})",
                        bound_violation.value,
                        bound_violation.lower_inclusive,
                        bound_violation.upper_inclusive,
                        bound_violation.violation_kind
                    ),
                },
            );
        }

        candidate_diagnostics.push(ReturnComponentsStageCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: optimizer_diagnostic.raw_objective_value,
            constraint_penalty: optimizer_diagnostic.constraint_penalty,
            penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
            constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
            constraint_violations: combined_constraint_violations,
            objective_metric_buckets: candidate_input.objective_metric_buckets,
            objective_terms: optimizer_diagnostic
                .objective_composition
                .objective_terms
                .clone(),
            candidate_families: candidate_input.candidate_families.clone(),
            candidate_provenance: candidate_input.candidate_provenance.clone(),
            return_distribution_diagnostic: candidate_input.return_distribution_diagnostic,
            jump_diagnostic: candidate_input.jump_diagnostic.clone(),
            gap_diagnostic: candidate_input.gap_diagnostic.clone(),
            activity_coupling_diagnostic: candidate_input.activity_coupling_diagnostic.clone(),
            price_range_sanity_diagnostic: candidate_input.price_range_sanity_diagnostic.clone(),
            dominant_scale_driver: candidate_input.dominant_scale_driver.clone(),
        });
    }

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = generated_candidates
        .candidate_inputs
        .iter()
        .position(|candidate| candidate.candidate_id == *selected_candidate_id)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate id was not found in generated candidate_inputs"
                    .to_string(),
            },
        );
    };

    let selected_candidate_input =
        &generated_candidates.candidate_inputs[selected_candidate_position];
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];
    let nig_configuration = match NigSharedPlusShiftConfiguration::try_new(
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .alpha,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .beta_0,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .mu,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    if !nig_configuration.delta_beta.is_finite() || nig_configuration.delta_beta >= 0.0 {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "nig_shared_plus_shift_configuration.delta_beta must be finite and strictly negative for return skew asymmetry".to_string(),
            },
        );
    }

    let news_process_configuration = match NewsProcessConfiguration::try_new(
        selected_candidate_input
            .news_process_configuration
            .arrivals
            .clone(),
        selected_candidate_input
            .news_process_configuration
            .shared_mark_law,
        selected_candidate_input
            .news_process_configuration
            .known_component_mark_scales,
        selected_candidate_input
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
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

    let fitted_stage_payload = ReturnComponentsStageFittedPayload {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: nig_configuration,
            news_process_configuration,
        },
        candidate_diagnostics,
        candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
        objective_weighting_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
        objective_metric_bucket_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
        candidate_family_counts: generated_candidates.candidate_family_counts.clone(),
        activity_input_transport: stage_input.activity_input.transport(),
        activity_selected_hawkes_candidate_id: match stage_input.activity_input.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => stage_input
                .activity_input
                .selected_hawkes_candidate_id()
                .map(ToOwned::to_owned),
            CalibrationStageHandoffTransport::PersistedArtifactReference => None,
        },
        activity_artifact_reference: stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: build_return_components_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ReturnComponentsStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            candidate_generation_seed,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: generated_candidates.candidate_inputs.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
            objective_weighting_policy:
                CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
            objective_metric_bucket_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
            candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
            candidate_family_counts: generated_candidates.candidate_family_counts,
        }),
        failure: None,
    }
}

fn generate_return_components_stage_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
    close_open_gap_detection_filter: &str,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
    stage_seed: u64,
) -> Result<
    ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    candidate_generation_input.validate()?;

    let sorted_signals = sort_candidate_generation_signals(
        candidate_generation_input,
        derived_close_open_gap_candidates,
    );
    let summary_statistics = summarize_candidate_generation_signals(
        candidate_generation_input,
        &sorted_signals,
        activity_calibration_context,
    );

    let jump_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        &RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES,
    );
    let gap_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.combined_gap_magnitudes.as_slice(),
        &RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES,
    );

    let candidate_family_sets = build_candidate_family_sets(
        candidate_generation_input,
        &sorted_signals,
        summary_statistics,
        jump_abs_return_bin_edges.as_slice(),
        gap_abs_return_bin_edges.as_slice(),
    );

    let candidate_provenance_template = build_candidate_provenance_template(
        sorted_signals.source_window,
        stage_seed,
        jump_abs_return_bin_edges,
        gap_abs_return_bin_edges,
        summary_statistics,
        close_open_gap_detection_filter,
    );
    let candidate_family_counts = build_candidate_family_counts(&candidate_family_sets);
    let generated_candidates = enumerate_candidate_inputs(
        candidate_generation_input,
        &candidate_family_sets,
        summary_statistics,
        &candidate_provenance_template,
    )?;

    Ok(ReturnComponentsStageCandidateGenerationOutput {
        candidate_inputs: generated_candidates,
        candidate_family_counts,
    })
}

fn resolve_activity_calibration_context(
    activity_input: &ReturnComponentsStageActivityInput,
) -> Option<ReturnComponentsStageActivityCalibrationContext> {
    let hawkes_payload = activity_input.hawkes_payload()?;
    let selected_candidate = hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)?;
    let modeled_open_event_rate_per_hour = selected_candidate
        .implied_metrics
        .model_implied_open_event_rate_per_hour;
    let representative_interarrival_hours = selected_candidate
        .source_regime
        .open_session_mean_interarrival_seconds
        / 3_600.0;

    if !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour <= 0.0
        || !representative_interarrival_hours.is_finite()
        || representative_interarrival_hours <= 0.0
    {
        return None;
    }

    Some(ReturnComponentsStageActivityCalibrationContext {
        modeled_open_event_rate_per_hour,
        representative_interarrival_hours,
    })
}

#[derive(Debug, Clone)]
struct ReturnComponentsStageSortedSignalInputs {
    known_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    observed_open_session_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    continuous_fit_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    known_sample_magnitudes: Vec<f64>,
    surprise_open_jump_magnitudes: Vec<f64>,
    combined_gap_magnitudes: Vec<f64>,
    source_window: ReturnComponentsStageSignalSourceWindow,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStagePreparedCandidateGenerationInput {
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    close_open_gap_detection_filter: String,
}

#[derive(Debug, Clone, Copy)]
struct ReturnComponentsStageActivityCalibrationContext {
    modeled_open_event_rate_per_hour: f64,
    representative_interarrival_hours: f64,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStageCandidateFamilySets {
    nig_candidates: Vec<NigSharedPlusShiftConfiguration>,
    known_arrival_families: Vec<ReturnComponentsKnownNewsArrivalCandidateFamily>,
    surprise_arrival_families: Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily>,
    return_jump_mark_scale_families: Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily>,
    activity_coupling_families: Vec<ReturnComponentsActivityCouplingCandidateFamily>,
}

#[derive(Debug, Clone, Copy)]
struct ReturnComponentsStageReturnDistributionSummary {
    mean_return: f64,
    volatility: f64,
    skewness: f64,
    excess_kurtosis: f64,
}

fn sort_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
) -> ReturnComponentsStageSortedSignalInputs {
    let mut known_return_samples = candidate_generation_input
        .realized_known_news_return_samples
        .clone();
    known_return_samples.sort_by_key(|sample| sample.event_timestamp_utc_nanos);

    let mut observed_open_session_returns = candidate_generation_input
        .observed_open_session_returns
        .clone();
    observed_open_session_returns.sort_by_key(|observation| observation.return_timestamp_utc_nanos);

    let mut intraday_jump_candidates = candidate_generation_input
        .detected_intraday_jump_candidates
        .clone();
    intraday_jump_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);

    let mut close_open_gap_candidates = derived_close_open_gap_candidates.to_vec();
    close_open_gap_candidates.sort_by_key(|candidate| {
        (
            candidate.close_timestamp_utc_nanos,
            candidate.open_timestamp_utc_nanos,
        )
    });

    let known_sample_magnitudes: Vec<f64> = known_return_samples
        .iter()
        .map(|sample| {
            sample
                .open_window_jump_return
                .abs()
                .max(sample.closed_window_gap_return.abs())
        })
        .collect();

    let surprise_open_jump_magnitudes: Vec<f64> = intraday_jump_candidates
        .iter()
        .map(|candidate| candidate.jump_return.abs())
        .collect();

    let combined_gap_magnitudes = {
        let mut magnitudes =
            Vec::with_capacity(known_return_samples.len() + close_open_gap_candidates.len());
        magnitudes.extend(
            known_return_samples
                .iter()
                .map(|sample| sample.closed_window_gap_return.abs()),
        );
        magnitudes.extend(
            close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return.abs()),
        );
        magnitudes
    };

    let jump_candidate_timestamps: BTreeSet<i64> = intraday_jump_candidates
        .iter()
        .map(|candidate| candidate.jump_timestamp_utc_nanos)
        .collect();
    let continuous_filter_threshold = (median_abs(
        observed_open_session_returns
            .iter()
            .map(|observation| observation.log_return)
            .collect::<Vec<_>>()
            .as_slice(),
    )
        * RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER)
        .max(1.0e-8);

    let mut continuous_fit_returns: Vec<ReturnComponentsObservedOpenSessionReturn> =
        observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| {
                !is_within_known_news_exclusion_window(
                    observation.return_timestamp_utc_nanos,
                    candidate_generation_input
                        .known_news_calendar
                        .scheduled_event_timestamps_utc_nanos
                        .as_slice(),
                ) && !jump_candidate_timestamps.contains(&observation.return_timestamp_utc_nanos)
                    && observation.log_return.abs() <= continuous_filter_threshold
            })
            .collect();
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| observation.log_return.abs() <= continuous_filter_threshold)
            .collect();
    }
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns.clone();
    }

    ReturnComponentsStageSortedSignalInputs {
        known_return_samples,
        observed_open_session_returns,
        continuous_fit_returns,
        intraday_jump_candidates,
        close_open_gap_candidates,
        known_sample_magnitudes,
        surprise_open_jump_magnitudes,
        combined_gap_magnitudes,
        source_window: candidate_generation_input.known_news_calendar.source_window,
    }
}

fn summarize_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> ReturnComponentsStageObservedSignalSummaryStatistics {
    let source_window_duration_hours = sorted_signals
        .source_window
        .duration_hours()
        .max(f64::EPSILON);
    let continuous_fit_business_time_scales =
        derive_continuous_fit_business_time_scales(sorted_signals, activity_calibration_context);
    let mean_continuous_business_time_scale =
        arithmetic_mean(continuous_fit_business_time_scales.as_slice()).max(1.0e-8);
    let continuous_distribution = summarize_return_distribution(
        sorted_signals
            .continuous_fit_returns
            .iter()
            .map(|observation| observation.log_return)
            .collect::<Vec<_>>()
            .as_slice(),
        mean_continuous_business_time_scale,
    );
    let observed_log_price_span = compute_observed_log_price_span(
        sorted_signals
            .observed_open_session_returns
            .iter()
            .map(|observation| {
                (
                    observation.return_timestamp_utc_nanos,
                    observation.log_return,
                )
            })
            .chain(
                sorted_signals
                    .close_open_gap_candidates
                    .iter()
                    .map(|candidate| (candidate.open_timestamp_utc_nanos, candidate.gap_return)),
            )
            .collect::<Vec<_>>()
            .as_slice(),
    );

    ReturnComponentsStageObservedSignalSummaryStatistics {
        known_calendar_event_count: candidate_generation_input
            .known_news_calendar
            .scheduled_event_timestamps_utc_nanos
            .len(),
        known_realized_sample_count: sorted_signals.known_return_samples.len(),
        source_window_duration_hours,
        observed_open_session_return_count: sorted_signals.observed_open_session_returns.len(),
        continuous_fit_return_count: sorted_signals.continuous_fit_returns.len(),
        intraday_jump_candidate_count: sorted_signals.intraday_jump_candidates.len(),
        close_open_gap_candidate_count: sorted_signals.close_open_gap_candidates.len(),
        open_session_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
        mean_continuous_business_time_scale,
        continuous_return_mean: continuous_distribution.mean_return,
        continuous_return_volatility: continuous_distribution.volatility,
        continuous_return_skewness: continuous_distribution.skewness,
        continuous_return_excess_kurtosis: continuous_distribution.excess_kurtosis,
        observed_log_price_span,
        mean_abs_known_open_jump_return: arithmetic_mean(
            sorted_signals
                .known_return_samples
                .iter()
                .map(|sample| sample.open_window_jump_return.abs())
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        mean_abs_known_closed_gap_return: arithmetic_mean(
            sorted_signals
                .known_return_samples
                .iter()
                .map(|sample| sample.closed_window_gap_return.abs())
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        mean_abs_surprise_open_jump_return: arithmetic_mean(
            sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        ),
        mean_abs_surprise_closed_gap_return: arithmetic_mean(
            sorted_signals
                .close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return.abs())
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        mean_known_post_shock_activity_lift: arithmetic_mean(
            sorted_signals
                .known_return_samples
                .iter()
                .map(|sample| sample.post_shock_activity_lift)
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        mean_surprise_post_shock_activity_lift: arithmetic_mean(
            sorted_signals
                .intraday_jump_candidates
                .iter()
                .map(|candidate| candidate.post_shock_activity_lift)
                .chain(
                    sorted_signals
                        .close_open_gap_candidates
                        .iter()
                        .map(|candidate| candidate.post_shock_activity_lift),
                )
                .collect::<Vec<_>>()
                .as_slice(),
        ),
        known_news_coverage_ratio: sorted_signals.known_return_samples.len() as f64
            / candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .len()
                .max(1) as f64,
        surprise_arrival_intensity_per_hour: (sorted_signals.intraday_jump_candidates.len()
            + sorted_signals.close_open_gap_candidates.len())
            as f64
            / source_window_duration_hours,
    }
}

fn build_candidate_family_sets(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    jump_abs_return_bin_edges: &[f64],
    gap_abs_return_bin_edges: &[f64],
) -> ReturnComponentsStageCandidateFamilySets {
    let nig_candidates = rank_and_truncate_candidates(
        build_nig_search_candidates(summary_statistics),
        RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES,
        |candidate| rank_nig_candidate(summary_statistics, candidate),
    );
    let known_arrival_families = build_known_arrival_families(
        candidate_generation_input,
        sorted_signals.known_sample_magnitudes.as_slice(),
        sorted_signals.known_return_samples.as_slice(),
    );
    let surprise_arrival_families = build_surprise_arrival_families(
        summary_statistics.surprise_arrival_intensity_per_hour,
        summary_statistics.intraday_jump_candidate_count
            + summary_statistics.close_open_gap_candidate_count,
    );
    let return_jump_mark_scale_families = rank_and_truncate_candidates(
        build_return_jump_mark_scale_families(
            candidate_generation_input,
            summary_statistics,
            jump_abs_return_bin_edges,
            gap_abs_return_bin_edges,
        ),
        RETURN_COMPONENTS_STAGE_MAX_RETURN_JUMP_MARK_SCALE_FAMILIES,
        |family| {
            rank_return_jump_mark_scale_family(
                summary_statistics,
                candidate_generation_input.shared_mark_law_template,
                family,
            )
        },
    );
    let activity_coupling_families = rank_and_truncate_candidates(
        build_activity_coupling_families(summary_statistics),
        RETURN_COMPONENTS_STAGE_MAX_ACTIVITY_COUPLING_FAMILIES,
        |family| rank_activity_coupling_family(summary_statistics, family),
    );

    ReturnComponentsStageCandidateFamilySets {
        nig_candidates,
        known_arrival_families,
        surprise_arrival_families,
        return_jump_mark_scale_families,
        activity_coupling_families,
    }
}

fn build_known_arrival_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    known_sample_magnitudes: &[f64],
    known_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
) -> Vec<ReturnComponentsKnownNewsArrivalCandidateFamily> {
    let known_magnitude_by_timestamp: BTreeMap<i64, f64> = known_return_samples
        .iter()
        .map(|sample| {
            (
                sample.event_timestamp_utc_nanos,
                sample
                    .open_window_jump_return
                    .abs()
                    .max(sample.closed_window_gap_return.abs()),
            )
        })
        .collect();

    let known_arrival_thresholds = deterministic_quantile_bins(
        known_sample_magnitudes,
        &RETURN_COMPONENTS_STAGE_KNOWN_RETENTION_QUANTILES,
    );

    known_arrival_thresholds
        .into_iter()
        .map(|threshold| {
            let retained_known_event_timestamps_utc_nanos = candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .iter()
                .copied()
                .filter(|timestamp_utc_nanos| {
                    known_magnitude_by_timestamp
                        .get(timestamp_utc_nanos)
                        .copied()
                        .unwrap_or(f64::MAX)
                        >= threshold
                })
                .collect();

            ReturnComponentsKnownNewsArrivalCandidateFamily {
                minimum_abs_known_return_threshold: threshold,
                retained_known_event_timestamps_utc_nanos,
            }
        })
        .collect()
}

fn build_surprise_arrival_families(
    surprise_arrival_intensity_per_hour: f64,
    surprise_evidence_count: usize,
) -> Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily> {
    if surprise_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        return vec![ReturnComponentsSurpriseNewsArrivalCandidateFamily {
            baseline_intensity: 0.0,
        }];
    }

    let mut surprise_intensity_candidates = deduplicate_sorted_f64_values(vec![
        round_candidate_value(surprise_arrival_intensity_per_hour * 0.75),
        round_candidate_value(surprise_arrival_intensity_per_hour),
        round_candidate_value(surprise_arrival_intensity_per_hour * 1.25),
    ]);
    if surprise_intensity_candidates.is_empty() {
        surprise_intensity_candidates.push(0.0);
    }

    surprise_intensity_candidates
        .into_iter()
        .map(
            |baseline_intensity| ReturnComponentsSurpriseNewsArrivalCandidateFamily {
                baseline_intensity,
            },
        )
        .collect()
}

fn build_return_jump_mark_scale_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    jump_abs_return_bin_edges: &[f64],
    gap_abs_return_bin_edges: &[f64],
) -> Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily> {
    let total_shock_evidence_count = summary_statistics.known_realized_sample_count
        + summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;
    if total_shock_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        return vec![ReturnComponentsReturnJumpMarkScaleCandidateFamily { sigma_g: 0.0 }];
    }

    let weighted_shock_magnitude_sum = summary_statistics.known_realized_sample_count as f64
        * summary_statistics.mean_abs_known_open_jump_return
        + summary_statistics.known_realized_sample_count as f64
            * summary_statistics.mean_abs_known_closed_gap_return
        + summary_statistics.intraday_jump_candidate_count as f64
            * arithmetic_mean(jump_abs_return_bin_edges)
                .max(summary_statistics.mean_abs_surprise_open_jump_return)
        + summary_statistics.close_open_gap_candidate_count as f64
            * arithmetic_mean(gap_abs_return_bin_edges)
                .max(summary_statistics.mean_abs_surprise_closed_gap_return);
    let weighted_shock_magnitude_count = (summary_statistics.known_realized_sample_count * 2
        + summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count)
        .max(1) as f64;
    let observed_mean_abs_shock_magnitude =
        weighted_shock_magnitude_sum / weighted_shock_magnitude_count;
    let shared_mark_multiplier = shared_mark_law_mean_abs_jump_multiplier(
        candidate_generation_input.shared_mark_law_template,
    )
    .max(1.0e-8);
    let sigma_g_target = observed_mean_abs_shock_magnitude / shared_mark_multiplier;
    let mut sigma_g_values = scale_candidates_from_target(sigma_g_target, false);
    if sigma_g_values.is_empty() {
        sigma_g_values.push(round_candidate_value(
            candidate_generation_input
                .shared_mark_law_template
                .coupled_gap_marks
                .sigma_g,
        ));
    }

    sigma_g_values
        .into_iter()
        .map(|sigma_g| ReturnComponentsReturnJumpMarkScaleCandidateFamily { sigma_g })
        .collect()
}

fn build_activity_coupling_families(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> Vec<ReturnComponentsActivityCouplingCandidateFamily> {
    let known_activity_values =
        scale_candidates_from_target(summary_statistics.mean_known_post_shock_activity_lift, true);
    let surprise_activity_values = if summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count
        < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
    {
        vec![0.0]
    } else {
        scale_candidates_from_target(
            summary_statistics.mean_surprise_post_shock_activity_lift,
            true,
        )
    };

    let mut families = Vec::new();
    for known_activity_coupling_scale in &known_activity_values {
        for surprise_activity_coupling_scale in &surprise_activity_values {
            families.push(ReturnComponentsActivityCouplingCandidateFamily {
                known_activity_coupling_scale: *known_activity_coupling_scale,
                surprise_activity_coupling_scale: *surprise_activity_coupling_scale,
            });
        }
    }

    families
}

fn build_candidate_provenance_template(
    source_window: ReturnComponentsStageSignalSourceWindow,
    stage_seed: u64,
    jump_abs_return_bin_edges: Vec<f64>,
    gap_abs_return_bin_edges: Vec<f64>,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    close_open_gap_detection_filter: &str,
) -> ReturnComponentsStageCandidateProvenance {
    ReturnComponentsStageCandidateProvenance {
        source_windows: vec![
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "known_news_calendar".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "realized_known_news_return_samples".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_open_session_returns".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "detected_intraday_jump_candidates".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_close_open_boundary_observations".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
        ],
        preprocessing_filter_policy: RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
        preprocessing_filters: vec![
            "reject_non_finite_known_news_returns".to_string(),
            "reject_non_finite_observed_open_session_returns".to_string(),
            "exclude_known_news_windows_from_continuous_nig_fit".to_string(),
            "exclude_repeated_and_isolated_material_shock_returns_from_continuous_nig_fit"
                .to_string(),
            "reject_non_finite_intraday_jump_returns".to_string(),
            "reject_non_finite_and_non_positive_close_open_boundary_prices".to_string(),
            "enforce_non_negative_post_shock_activity_lift".to_string(),
            "sort_observed_open_session_returns_by_timestamp".to_string(),
            "sort_intraday_jumps_by_jump_timestamp".to_string(),
            "validate_close_open_session_boundary_transitions".to_string(),
            close_open_gap_detection_filter.to_string(),
            "sort_close_open_gaps_by_close_then_open_timestamp".to_string(),
            "deterministic_nearest_rank_quantile_binning".to_string(),
        ],
        jump_abs_return_bin_edges,
        gap_abs_return_bin_edges,
        summary_statistics,
        deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata {
            stage_seed,
            candidate_generation_seed: stage_seed,
            deterministic_seed_policy: RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
        },
    }
}

fn build_candidate_family_counts(
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
) -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: candidate_family_sets.nig_candidates.len(),
        known_news_arrival_family_count: candidate_family_sets.known_arrival_families.len(),
        surprise_news_arrival_family_count: candidate_family_sets.surprise_arrival_families.len(),
        return_jump_mark_scale_family_count: candidate_family_sets
            .return_jump_mark_scale_families
            .len(),
        activity_coupling_family_count: candidate_family_sets.activity_coupling_families.len(),
    }
}

fn build_nig_search_candidates(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> Vec<NigSharedPlusShiftConfiguration> {
    let kurtosis_magnitude = summary_statistics.continuous_return_excess_kurtosis.abs();
    let beta_center = clamp(
        -summary_statistics.continuous_return_skewness * 0.20
            + (summary_statistics.open_session_sell_share_proxy - 0.5) * 0.30,
        -0.75,
        0.75,
    );
    let delta_beta_center = -clamp(
        0.05 + summary_statistics.continuous_return_skewness.abs() * 0.10
            + (summary_statistics.open_session_sell_share_proxy - 0.5).abs() * 0.20,
        0.05,
        0.60,
    );
    let volatility_target = summary_statistics.continuous_return_volatility.max(1.0e-6);
    let variance_target = volatility_target * volatility_target;
    let mean_target = summary_statistics.continuous_return_mean;

    let alpha_values = unique_sorted_signed(&[
        1.5,
        2.0,
        3.0,
        5.0,
        8.0,
        12.0,
        clamp(2.0 + kurtosis_magnitude * 0.5, 1.5, 18.0),
    ])
    .into_iter()
    .filter(|value| *value > 0.0)
    .collect::<Vec<_>>();
    let beta_values = unique_sorted_signed(&[
        beta_center - 0.25,
        beta_center - 0.10,
        beta_center,
        beta_center + 0.10,
        beta_center + 0.25,
    ]);
    let delta_beta_values = unique_sorted_signed(&[
        delta_beta_center * 0.5,
        delta_beta_center * 0.75,
        delta_beta_center,
        delta_beta_center * 1.25,
    ]);
    let mu_values = unique_sorted_signed(&[
        mean_target - volatility_target * 0.10,
        mean_target,
        mean_target + volatility_target * 0.10,
    ]);

    let mut candidates = Vec::new();
    for alpha in alpha_values {
        for beta_0 in &beta_values {
            for delta_beta in &delta_beta_values {
                let margin = alpha - beta_0.abs().max((beta_0 + delta_beta).abs());
                if margin <= 0.25 {
                    continue;
                }

                let variance_per_unit = theoretical_mixture_distribution_diagnostic(
                    NigSharedPlusShiftConfiguration {
                        alpha,
                        beta_0: *beta_0,
                        delta: 1.0,
                        mu: 0.0,
                        delta_beta: *delta_beta,
                    },
                    summary_statistics.open_session_sell_share_proxy,
                )
                .map(|diagnostic| {
                    let fitted_volatility = diagnostic.fitted_return_volatility.max(1.0e-8);
                    fitted_volatility * fitted_volatility
                })
                .unwrap_or(1.0);
                let delta_center = clamp(
                    variance_target / variance_per_unit.max(1.0e-10),
                    RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
                    25.0,
                );
                let delta_values = unique_sorted_signed(&[
                    delta_center * 0.50,
                    delta_center * 0.75,
                    delta_center,
                    delta_center * 1.25,
                    delta_center * 1.50,
                ])
                .into_iter()
                .filter(|value| *value > 0.0)
                .collect::<Vec<_>>();

                for delta in &delta_values {
                    for mu in &mu_values {
                        if let Ok(configuration) = NigSharedPlusShiftConfiguration::try_new(
                            round_positive_candidate_value(alpha),
                            round_signed_candidate_value(*beta_0),
                            round_positive_candidate_value(*delta),
                            round_signed_candidate_value(*mu),
                            round_negative_candidate_value(*delta_beta),
                        ) {
                            candidates.push(configuration);
                        }
                    }
                }
            }
        }
    }

    let mut unique_candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        let candidate_key = format!(
            "{:.6}|{:.6}|{:.12}|{:.6}|{:.6}",
            candidate.alpha, candidate.beta_0, candidate.delta, candidate.mu, candidate.delta_beta
        );
        if seen.insert(candidate_key) {
            unique_candidates.push(candidate);
        }
    }

    unique_candidates
}

fn rank_and_truncate_candidates<T, F>(
    candidates: Vec<T>,
    maximum_count: usize,
    mut score_candidate: F,
) -> Vec<T>
where
    F: FnMut(&T) -> f64,
{
    let retained_count = maximum_count.max(1);
    let mut ranked_candidates = candidates
        .into_iter()
        .enumerate()
        .map(|(position, candidate)| (score_candidate(&candidate), position, candidate))
        .collect::<Vec<_>>();
    ranked_candidates.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    ranked_candidates.truncate(retained_count);
    ranked_candidates
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn rank_nig_candidate(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    candidate: &NigSharedPlusShiftConfiguration,
) -> f64 {
    theoretical_mixture_distribution_diagnostic(
        *candidate,
        summary_statistics.open_session_sell_share_proxy,
    )
    .map(|diagnostic| score_continuous_distribution_fit(diagnostic, summary_statistics))
    .unwrap_or(f64::INFINITY)
}

fn rank_return_jump_mark_scale_family(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> f64 {
    let fitted_mean_abs_jump_return = fitted_mean_abs_jump_return(shared_mark_law_template, family);

    zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        summary_statistics.mean_abs_known_open_jump_return,
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_closed_gap_return
        },
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        summary_statistics.mean_abs_known_closed_gap_return,
        1.0e-6,
    )
}

fn rank_activity_coupling_family(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> f64 {
    let surprise_evidence_count = summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;

    zero_aware_relative_error(
        family.known_activity_coupling_scale,
        summary_statistics.mean_known_post_shock_activity_lift,
        1.0e-6,
    ) + zero_aware_relative_error(
        family.surprise_activity_coupling_scale,
        if surprise_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
            0.0
        } else {
            summary_statistics.mean_surprise_post_shock_activity_lift
        },
        1.0e-6,
    )
}

fn scale_candidates_from_target(target_value: f64, include_zero: bool) -> Vec<f64> {
    if target_value <= 0.0 || !target_value.is_finite() {
        return vec![0.0];
    }

    let mut values = deduplicate_sorted_f64_values(vec![
        if include_zero {
            0.0
        } else {
            target_value * 0.75
        },
        round_candidate_value(target_value * 0.75),
        round_candidate_value(target_value),
        round_candidate_value(target_value * 1.25),
    ]);
    if values.is_empty() {
        values.push(round_candidate_value(target_value));
    }

    values
}

fn derive_continuous_fit_business_time_scales(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> Vec<f64> {
    let Some(activity_calibration_context) = activity_calibration_context else {
        return vec![1.0; sorted_signals.continuous_fit_returns.len()];
    };

    let representative_interarrival_hours = activity_calibration_context
        .representative_interarrival_hours
        .max(1.0e-8);
    let maximum_elapsed_hours = representative_interarrival_hours * 8.0;
    let fallback_business_time_scale = (activity_calibration_context
        .modeled_open_event_rate_per_hour
        * representative_interarrival_hours)
        .max(1.0e-8);
    let mut business_time_scale_by_timestamp = BTreeMap::new();
    let mut previous_timestamp_utc_nanos: Option<i64> = None;

    for observation in &sorted_signals.observed_open_session_returns {
        let elapsed_hours = previous_timestamp_utc_nanos
            .map(|previous_timestamp_utc_nanos_value| {
                (observation.return_timestamp_utc_nanos - previous_timestamp_utc_nanos_value) as f64
                    / 3_600_000_000_000.0
            })
            .filter(|elapsed_hours_value| {
                elapsed_hours_value.is_finite()
                    && *elapsed_hours_value > 0.0
                    && *elapsed_hours_value <= maximum_elapsed_hours
            })
            .unwrap_or(representative_interarrival_hours);
        let business_time_scale = (activity_calibration_context.modeled_open_event_rate_per_hour
            * elapsed_hours)
            .max(1.0e-8);

        business_time_scale_by_timestamp
            .insert(observation.return_timestamp_utc_nanos, business_time_scale);

        previous_timestamp_utc_nanos = Some(observation.return_timestamp_utc_nanos);
    }

    sorted_signals
        .continuous_fit_returns
        .iter()
        .map(|observation| {
            business_time_scale_by_timestamp
                .get(&observation.return_timestamp_utc_nanos)
                .copied()
                .unwrap_or(fallback_business_time_scale)
        })
        .collect()
}

fn shared_mark_law_mean_abs_jump_multiplier(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> f64 {
    let severity_scale_proxy = (shared_mark_law_template.gamma_severity_marks.shape_k
        * shared_mark_law_template.gamma_severity_marks.scale_theta)
        .sqrt();
    let base_shock_mean_absolute_value = shared_mark_law_template
        .double_exponential_base_shock
        .probability_positive
        / shared_mark_law_template
            .double_exponential_base_shock
            .eta_plus
        + (1.0
            - shared_mark_law_template
                .double_exponential_base_shock
                .probability_positive)
            / shared_mark_law_template
                .double_exponential_base_shock
                .eta_minus;

    severity_scale_proxy * base_shock_mean_absolute_value
}

fn fitted_mean_abs_jump_return(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> f64 {
    return_jump_family.sigma_g * shared_mark_law_mean_abs_jump_multiplier(shared_mark_law_template)
}

fn summarize_return_distribution(
    values: &[f64],
    mean_business_time_scale: f64,
) -> ReturnComponentsStageReturnDistributionSummary {
    let normalized_business_time_scale = mean_business_time_scale.max(1.0e-8);
    ReturnComponentsStageReturnDistributionSummary {
        mean_return: arithmetic_mean_signed(values) / normalized_business_time_scale,
        volatility: sample_standard_deviation(values) / normalized_business_time_scale.sqrt(),
        skewness: sample_adjusted_skewness(values).unwrap_or(0.0)
            * normalized_business_time_scale.sqrt(),
        excess_kurtosis: sample_adjusted_excess_kurtosis(values).unwrap_or(0.0)
            * normalized_business_time_scale,
    }
}

fn compute_observed_log_price_span(timestamped_returns: &[(i64, f64)]) -> f64 {
    if timestamped_returns.is_empty() {
        return 0.0;
    }

    let mut sorted_returns = timestamped_returns.to_vec();
    sorted_returns.sort_by_key(|(timestamp_utc_nanos, _)| *timestamp_utc_nanos);

    let mut cumulative_return = 0.0_f64;
    let mut minimum_cumulative_return = 0.0_f64;
    let mut maximum_cumulative_return = 0.0_f64;
    for (_, return_increment) in sorted_returns {
        cumulative_return += return_increment;
        minimum_cumulative_return = minimum_cumulative_return.min(cumulative_return);
        maximum_cumulative_return = maximum_cumulative_return.max(cumulative_return);
    }

    (maximum_cumulative_return - minimum_cumulative_return).max(0.0)
}

fn is_within_known_news_exclusion_window(
    candidate_timestamp_utc_nanos: i64,
    known_news_timestamps: &[i64],
) -> bool {
    if known_news_timestamps.is_empty() {
        return false;
    }

    let window_nanos =
        Duration::minutes(RETURN_COMPONENTS_STAGE_KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES)
            .num_nanoseconds()
            .expect("known-news exclusion window must fit in i64");
    let lower_bound = candidate_timestamp_utc_nanos - window_nanos;
    let upper_bound = candidate_timestamp_utc_nanos + window_nanos;
    let lower_index = known_news_timestamps.partition_point(|timestamp| *timestamp < lower_bound);

    lower_index < known_news_timestamps.len() && known_news_timestamps[lower_index] <= upper_bound
}

fn median_abs(values: &[f64]) -> f64 {
    let mut retained_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();
    if retained_values.is_empty() {
        return 0.0;
    }

    retained_values.sort_by(f64::total_cmp);
    let midpoint = retained_values.len() / 2;
    if retained_values.len() % 2 == 0 {
        (retained_values[midpoint - 1] + retained_values[midpoint]) / 2.0
    } else {
        retained_values[midpoint]
    }
}

fn clamp(value: f64, lower_bound: f64, upper_bound: f64) -> f64 {
    value.max(lower_bound).min(upper_bound)
}

fn prepare_candidate_generation_input(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
) -> Result<
    ReturnComponentsStagePreparedCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let derived_close_open_gap_candidates = derive_close_open_gap_candidates(
        exchange_code,
        calendar_registry,
        candidate_generation_input.known_news_calendar.source_window,
        candidate_generation_input
            .observed_close_open_boundary_observations
            .as_slice(),
        candidate_generation_input.close_open_gap_detection_policy,
    )?;
    let close_open_gap_detection_filter = format!(
        "derive_close_open_log_gap_candidates_abs_gt_{:.6}",
        candidate_generation_input
            .close_open_gap_detection_policy
            .material_gap_absolute_return_tolerance
    );

    Ok(ReturnComponentsStagePreparedCandidateGenerationInput {
        candidate_generation_input,
        derived_close_open_gap_candidates,
        close_open_gap_detection_filter,
    })
}

fn derive_close_open_gap_candidates(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    source_window: ReturnComponentsStageSignalSourceWindow,
    observed_close_open_boundary_observations: &[ReturnComponentsObservedCloseOpenBoundaryObservation],
    detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy,
) -> Result<
    Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut derived_close_open_gap_candidates = Vec::new();

    for observed_boundary in observed_close_open_boundary_observations {
        observed_boundary.validate(source_window)?;
        validate_close_open_boundary_transition(
            exchange_code,
            calendar_registry,
            observed_boundary.close_timestamp_utc_nanos,
            observed_boundary.open_timestamp_utc_nanos,
        )?;

        let gap_return = (observed_boundary.open_price / observed_boundary.close_price).ln();
        if gap_return.abs() <= detection_policy.material_gap_absolute_return_tolerance {
            continue;
        }

        let derived_candidate = ReturnComponentsDetectedCloseOpenGapCandidate {
            close_timestamp_utc_nanos: observed_boundary.close_timestamp_utc_nanos,
            open_timestamp_utc_nanos: observed_boundary.open_timestamp_utc_nanos,
            gap_return,
            post_shock_activity_lift: observed_boundary.post_shock_activity_lift,
        };
        derived_candidate.validate(source_window)?;
        derived_close_open_gap_candidates.push(derived_candidate);
    }

    Ok(derived_close_open_gap_candidates)
}

fn validate_close_open_boundary_transition(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    close_timestamp_utc_nanos: i64,
    open_timestamp_utc_nanos: i64,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    let close_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        close_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let close_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, close_timestamp_utc_nanos)?;
    if close_previous_state != MarketState::Open || close_boundary_state != MarketState::Closed {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                close_timestamp_utc_nanos,
                previous_state: close_previous_state,
                boundary_state: close_boundary_state,
            },
        );
    }

    let open_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        open_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let open_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, open_timestamp_utc_nanos)?;
    if open_previous_state != MarketState::Closed || open_boundary_state != MarketState::Open {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidOpenBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                open_timestamp_utc_nanos,
                previous_state: open_previous_state,
                boundary_state: open_boundary_state,
            },
        );
    }

    Ok(())
}

fn resolve_market_state_at_nanos(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    timestamp_utc_nanos: i64,
) -> Result<MarketState, ReturnComponentsStageCandidateGenerationInputError> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);

    calendar_registry
        .resolve_market_state_at_utc(exchange_code, timestamp_utc)
        .map_err(|source: crate::calendar::ExchangeCalendarResolveError| {
            ReturnComponentsStageCandidateGenerationInputError::CloseOpenBoundaryStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos,
                error_message: source.to_string(),
            }
        })
}

fn enumerate_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    candidate_provenance_template: &ReturnComponentsStageCandidateProvenance,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut generated_candidates = Vec::new();

    for (nig_position, nig_candidate) in candidate_family_sets.nig_candidates.iter().enumerate() {
        for (known_arrival_position, known_arrival_family) in candidate_family_sets
            .known_arrival_families
            .iter()
            .enumerate()
        {
            for (surprise_arrival_position, surprise_arrival_family) in candidate_family_sets
                .surprise_arrival_families
                .iter()
                .enumerate()
            {
                for (return_jump_position, return_jump_family) in candidate_family_sets
                    .return_jump_mark_scale_families
                    .iter()
                    .enumerate()
                {
                    for (activity_coupling_position, activity_coupling_family) in
                        candidate_family_sets
                            .activity_coupling_families
                            .iter()
                            .enumerate()
                    {
                        let candidate_families = ReturnComponentsStageCandidateFamilies {
                            known_news_arrival_controls: known_arrival_family.clone(),
                            surprise_news_arrival_controls: *surprise_arrival_family,
                            return_jump_mark_scale_controls: *return_jump_family,
                            news_to_hawkes_activity_coupling_controls: *activity_coupling_family,
                        };

                        let mut return_distribution_diagnostic =
                            theoretical_mixture_distribution_diagnostic(
                                *nig_candidate,
                                summary_statistics.open_session_sell_share_proxy,
                            )
                            .unwrap_or(
                                ReturnComponentsStageReturnDistributionDiagnostic {
                                    sample_count: summary_statistics.continuous_fit_return_count,
                                    observed_mean_return: summary_statistics.continuous_return_mean,
                                    fitted_mean_return: 0.0,
                                    observed_return_volatility: summary_statistics
                                        .continuous_return_volatility,
                                    fitted_return_volatility: 0.0,
                                    observed_skewness: summary_statistics
                                        .continuous_return_skewness,
                                    fitted_skewness: 0.0,
                                    observed_excess_kurtosis: summary_statistics
                                        .continuous_return_excess_kurtosis,
                                    fitted_excess_kurtosis: 0.0,
                                },
                            );
                        return_distribution_diagnostic.sample_count =
                            summary_statistics.continuous_fit_return_count;
                        return_distribution_diagnostic.observed_mean_return =
                            summary_statistics.continuous_return_mean;
                        return_distribution_diagnostic.observed_return_volatility =
                            summary_statistics.continuous_return_volatility;
                        return_distribution_diagnostic.observed_skewness =
                            summary_statistics.continuous_return_skewness;
                        return_distribution_diagnostic.observed_excess_kurtosis =
                            summary_statistics.continuous_return_excess_kurtosis;
                        let jump_diagnostic = build_jump_diagnostic(
                            summary_statistics,
                            surprise_arrival_family,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let gap_diagnostic = build_gap_diagnostic(
                            summary_statistics,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
                            summary_statistics,
                            activity_coupling_family,
                        );
                        let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
                            summary_statistics,
                            return_distribution_diagnostic,
                            surprise_arrival_family,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let objective_metric_buckets = score_candidate_metric_buckets(
                            return_distribution_diagnostic,
                            &jump_diagnostic,
                            &gap_diagnostic,
                            &activity_coupling_diagnostic,
                            &price_range_sanity_diagnostic,
                            summary_statistics,
                        );
                        let dominant_scale_driver =
                            dominant_scale_driver_from_metric_buckets(objective_metric_buckets)
                                .to_string();
                        let objective_terms = build_candidate_objective_terms(
                            candidate_generation_input.objective_metric_weights,
                            objective_metric_buckets,
                        );
                        let candidate_id = format!(
                            "generated_nig_{:03}_known_{:03}_surprise_{:03}_mark_{:03}_coupling_{:03}",
                            nig_position + 1,
                            known_arrival_position + 1,
                            surprise_arrival_position + 1,
                            return_jump_position + 1,
                            activity_coupling_position + 1,
                        );

                        let news_process_configuration =
                            build_news_process_configuration_from_candidate_families(
                                candidate_generation_input,
                                known_arrival_family,
                                surprise_arrival_family,
                                return_jump_family,
                                activity_coupling_family,
                            )?;

                        generated_candidates.push(ReturnComponentsStageCandidateInput {
                            candidate_id,
                            nig_shared_plus_shift_configuration: *nig_candidate,
                            news_process_configuration,
                            objective_metric_buckets,
                            return_distribution_diagnostic,
                            jump_diagnostic,
                            gap_diagnostic,
                            activity_coupling_diagnostic,
                            price_range_sanity_diagnostic,
                            dominant_scale_driver,
                            objective_terms,
                            candidate_families,
                            candidate_provenance: candidate_provenance_template.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(generated_candidates)
}

fn build_candidate_objective_terms(
    objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> Vec<ReturnComponentsStageObjectiveTermInput> {
    vec![
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT.to_string(),
            weight: objective_metric_weights.continuous_distribution_fit_weight,
            observed_score: objective_metric_buckets.continuous_distribution_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT.to_string(),
            weight: objective_metric_weights.jump_fit_weight,
            observed_score: objective_metric_buckets.open_jump_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT.to_string(),
            weight: objective_metric_weights.gap_fit_weight,
            observed_score: objective_metric_buckets.closed_gap_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT.to_string(),
            weight: objective_metric_weights.activity_coupling_fit_weight,
            observed_score: objective_metric_buckets.activity_coupling_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT.to_string(),
            weight: objective_metric_weights.price_range_sanity_fit_weight,
            observed_score: objective_metric_buckets.price_range_sanity_fit,
        },
    ]
}

fn build_news_process_configuration_from_candidate_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    known_arrival_family: &ReturnComponentsKnownNewsArrivalCandidateFamily,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> Result<NewsProcessConfiguration, ReturnComponentsStageCandidateGenerationInputError> {
    let mut shared_mark_law = candidate_generation_input.shared_mark_law_template;
    shared_mark_law.coupled_gap_marks =
        CoupledGapMarkConfiguration::new(return_jump_family.sigma_g);

    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: known_arrival_family
                    .retained_known_event_timestamps_utc_nanos
                    .clone(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: surprise_arrival_family.baseline_intensity,
            },
        },
        shared_mark_law,
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.known_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.surprise_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
    )
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
            error_message: source.to_string(),
        }
    })
}

fn score_candidate_metric_buckets(
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    jump_diagnostic: &ReturnComponentsStageJumpDiagnostic,
    gap_diagnostic: &ReturnComponentsStageGapDiagnostic,
    activity_coupling_diagnostic: &ReturnComponentsStageActivityCouplingDiagnostic,
    price_range_sanity_diagnostic: &ReturnComponentsStagePriceRangeSanityDiagnostic,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> ReturnComponentsStageObjectiveMetricBuckets {
    let continuous_distribution_fit =
        score_continuous_distribution_fit(return_distribution_diagnostic, summary_statistics);
    let open_jump_fit = zero_aware_relative_error(
        jump_diagnostic.fitted_surprise_arrival_intensity_per_hour,
        jump_diagnostic.observed_jump_frequency_per_hour,
        1.0e-8,
    ) + zero_aware_relative_error(
        jump_diagnostic.fitted_mean_abs_jump_return,
        jump_diagnostic.observed_mean_abs_jump_return,
        1.0e-6,
    );
    let closed_gap_fit = zero_aware_relative_error(
        gap_diagnostic.fitted_mean_abs_gap_return,
        gap_diagnostic.observed_mean_abs_gap_return,
        1.0e-6,
    );
    let activity_coupling_fit = zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_known_activity_coupling_scale,
        activity_coupling_diagnostic.observed_known_post_shock_activity_lift,
        1.0e-6,
    ) + zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_surprise_activity_coupling_scale,
        activity_coupling_diagnostic.observed_surprise_post_shock_activity_lift,
        1.0e-6,
    );

    ReturnComponentsStageObjectiveMetricBuckets {
        continuous_distribution_fit: round_candidate_value(continuous_distribution_fit),
        open_jump_fit: round_candidate_value(open_jump_fit),
        closed_gap_fit: round_candidate_value(closed_gap_fit),
        activity_coupling_fit: round_candidate_value(activity_coupling_fit),
        price_range_sanity_fit: round_candidate_value(
            if price_range_sanity_diagnostic.is_flagged {
                price_range_sanity_diagnostic.expansion_ratio
                    / price_range_sanity_diagnostic.threshold_ratio.max(1.0)
            } else {
                0.0
            },
        ),
    }
}

fn score_continuous_distribution_fit(
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> f64 {
    0.15 * zero_aware_relative_error(
        return_distribution_diagnostic.fitted_mean_return,
        summary_statistics.continuous_return_mean,
        summary_statistics.continuous_return_volatility * 0.10 + 1.0e-8,
    ) + 0.35
        * zero_aware_relative_error(
            return_distribution_diagnostic.fitted_return_volatility,
            summary_statistics.continuous_return_volatility,
            1.0e-8,
        )
        + 0.25
            * zero_aware_relative_error(
                return_distribution_diagnostic.fitted_skewness,
                summary_statistics.continuous_return_skewness,
                0.25,
            )
        + 0.25
            * zero_aware_relative_error(
                return_distribution_diagnostic.fitted_excess_kurtosis,
                summary_statistics.continuous_return_excess_kurtosis,
                1.0,
            )
}

fn build_jump_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageJumpDiagnostic {
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageJumpDiagnostic {
        evidence_state: evidence_state_from_count(summary_statistics.intraday_jump_candidate_count)
            .to_string(),
        observed_jump_count: summary_statistics.intraday_jump_candidate_count,
        observed_jump_frequency_per_hour: if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.surprise_arrival_intensity_per_hour
        },
        observed_mean_abs_jump_return: if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        fitted_mean_abs_jump_return,
        fitted_sigma_g: return_jump_family.sigma_g,
        fitted_surprise_arrival_intensity_per_hour: surprise_arrival_family.baseline_intensity,
    }
}

fn build_gap_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageGapDiagnostic {
    let fitted_mean_abs_gap_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageGapDiagnostic {
        evidence_state: evidence_state_from_count(
            summary_statistics.close_open_gap_candidate_count,
        )
        .to_string(),
        observed_gap_count: summary_statistics.close_open_gap_candidate_count,
        observed_gap_frequency_per_hour: if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.close_open_gap_candidate_count as f64
                / summary_statistics.known_calendar_event_count.max(1) as f64
        },
        observed_mean_abs_gap_return: if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_closed_gap_return
        },
        fitted_mean_abs_gap_return,
        fitted_sigma_g: return_jump_family.sigma_g,
    }
}

fn build_activity_coupling_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsStageActivityCouplingDiagnostic {
    let surprise_evidence_count = summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;

    ReturnComponentsStageActivityCouplingDiagnostic {
        evidence_state: evidence_state_from_count(surprise_evidence_count).to_string(),
        observed_known_post_shock_activity_lift: summary_statistics
            .mean_known_post_shock_activity_lift,
        observed_surprise_post_shock_activity_lift: if surprise_evidence_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_surprise_post_shock_activity_lift
        },
        fitted_known_activity_coupling_scale: activity_coupling_family
            .known_activity_coupling_scale,
        fitted_surprise_activity_coupling_scale: activity_coupling_family
            .surprise_activity_coupling_scale,
    }
}

fn build_price_range_sanity_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStagePriceRangeSanityDiagnostic {
    let continuous_reference_span = 6.0
        * return_distribution_diagnostic.fitted_return_volatility
        * (summary_statistics.observed_open_session_return_count as f64
            * summary_statistics.mean_continuous_business_time_scale)
            .sqrt()
            .max(1.0);
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);
    let fitted_reference_log_price_span = continuous_reference_span
        + (summary_statistics.known_realized_sample_count as f64
            + surprise_arrival_family.baseline_intensity
                * summary_statistics.source_window_duration_hours)
            * fitted_mean_abs_jump_return
        + summary_statistics.close_open_gap_candidate_count as f64 * fitted_mean_abs_jump_return;
    let expansion_ratio =
        fitted_reference_log_price_span / summary_statistics.observed_log_price_span.max(1.0e-8);
    let is_flagged = expansion_ratio > RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO
        && fitted_reference_log_price_span > summary_statistics.observed_log_price_span + 0.05;

    ReturnComponentsStagePriceRangeSanityDiagnostic {
        is_flagged,
        observed_log_price_span: summary_statistics.observed_log_price_span,
        fitted_reference_log_price_span,
        expansion_ratio,
        threshold_ratio: RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO,
    }
}

fn dominant_scale_driver_from_metric_buckets(
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> &'static str {
    let mut dominant_driver = "nig_scale";
    let mut dominant_value = objective_metric_buckets.continuous_distribution_fit;

    for (driver, value) in [
        ("jump_scales", objective_metric_buckets.open_jump_fit),
        ("gap_behavior", objective_metric_buckets.closed_gap_fit),
        (
            "activity_coupling",
            objective_metric_buckets.activity_coupling_fit,
        ),
        (
            "price_range_sanity",
            objective_metric_buckets.price_range_sanity_fit,
        ),
    ] {
        if value > dominant_value {
            dominant_driver = driver;
            dominant_value = value;
        }
    }

    dominant_driver
}

fn evidence_state_from_count(event_count: usize) -> &'static str {
    if event_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        "insufficient_evidence"
    } else {
        "identified_component"
    }
}

fn theoretical_mixture_distribution_diagnostic(
    configuration: NigSharedPlusShiftConfiguration,
    open_session_sell_share_proxy: f64,
) -> Option<ReturnComponentsStageReturnDistributionDiagnostic> {
    let (mean_return, variance, skewness, excess_kurtosis) =
        theoretical_mixture_moments(configuration, open_session_sell_share_proxy)?;
    let fitted_return_volatility = variance.sqrt();

    Some(ReturnComponentsStageReturnDistributionDiagnostic {
        sample_count: 0,
        observed_mean_return: 0.0,
        fitted_mean_return: mean_return,
        observed_return_volatility: 0.0,
        fitted_return_volatility,
        observed_skewness: 0.0,
        fitted_skewness: skewness,
        observed_excess_kurtosis: 0.0,
        fitted_excess_kurtosis: excess_kurtosis,
    })
}

fn theoretical_mixture_moments(
    configuration: NigSharedPlusShiftConfiguration,
    open_session_sell_share_proxy: f64,
) -> Option<(f64, f64, f64, f64)> {
    let buy_raw_moments = nig_raw_moments(
        configuration.alpha,
        configuration.beta_0,
        configuration.delta,
        configuration.mu,
    )?;
    let sell_raw_moments = nig_raw_moments(
        configuration.alpha,
        configuration.derived_sell_beta(),
        configuration.delta,
        configuration.mu,
    )?;
    let negative_sell_raw_moments = (
        -sell_raw_moments.0,
        sell_raw_moments.1,
        -sell_raw_moments.2,
        sell_raw_moments.3,
    );
    let sell_weight = open_session_sell_share_proxy.clamp(0.0, 1.0);
    let buy_weight = 1.0 - sell_weight;

    let raw_mean = buy_weight * buy_raw_moments.0 + sell_weight * negative_sell_raw_moments.0;
    let raw_second = buy_weight * buy_raw_moments.1 + sell_weight * negative_sell_raw_moments.1;
    let raw_third = buy_weight * buy_raw_moments.2 + sell_weight * negative_sell_raw_moments.2;
    let raw_fourth = buy_weight * buy_raw_moments.3 + sell_weight * negative_sell_raw_moments.3;

    let variance = raw_second - raw_mean * raw_mean;
    if !variance.is_finite() || variance <= 1.0e-12 {
        return None;
    }

    let third_central_moment =
        raw_third - 3.0 * raw_mean * raw_second + 2.0 * raw_mean * raw_mean * raw_mean;
    let fourth_central_moment = raw_fourth - 4.0 * raw_mean * raw_third
        + 6.0 * raw_mean * raw_mean * raw_second
        - 3.0 * raw_mean.powi(4);
    let skewness = third_central_moment / variance.powf(1.5);
    let excess_kurtosis = fourth_central_moment / (variance * variance) - 3.0;

    Some((raw_mean, variance, skewness, excess_kurtosis))
}

fn nig_raw_moments(alpha: f64, beta: f64, delta: f64, mu: f64) -> Option<(f64, f64, f64, f64)> {
    let gamma_squared = alpha * alpha - beta * beta;
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return None;
    }
    let gamma = gamma_squared.sqrt();
    let kappa_1 = mu + delta * beta / gamma;
    let kappa_2 = delta * alpha * alpha / gamma.powi(3);
    let kappa_3 = 3.0 * delta * beta * alpha * alpha / gamma.powi(5);
    let kappa_4 = 3.0 * delta * alpha * alpha * (alpha * alpha + 4.0 * beta * beta) / gamma.powi(7);

    let raw_second = kappa_2 + kappa_1 * kappa_1;
    let raw_third = kappa_3 + 3.0 * kappa_2 * kappa_1 + kappa_1.powi(3);
    let raw_fourth = kappa_4
        + 4.0 * kappa_3 * kappa_1
        + 3.0 * kappa_2 * kappa_2
        + 6.0 * kappa_2 * kappa_1 * kappa_1
        + kappa_1.powi(4);

    Some((kappa_1, raw_second, raw_third, raw_fourth))
}

fn zero_aware_relative_error(observed: f64, target: f64, zero_floor: f64) -> f64 {
    let denominator = if target.abs() <= zero_floor {
        zero_floor
    } else {
        target.abs()
    };
    ((observed - target).abs() / denominator).max(0.0)
}

fn arithmetic_mean_signed(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.is_empty() {
        return 0.0;
    }

    finite_values.iter().sum::<f64>() / finite_values.len() as f64
}

fn sample_standard_deviation(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.len() < 2 {
        return finite_values.first().copied().unwrap_or(0.0).abs();
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let sum_squared_deviation = finite_values
        .iter()
        .map(|value| {
            let centered_value = *value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();

    (sum_squared_deviation / (finite_values.len() - 1) as f64)
        .max(0.0)
        .sqrt()
}

fn sample_adjusted_skewness(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let sample_count = finite_values.len();
    if sample_count < 3 {
        return None;
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let (sum_squared_deviation, sum_cubed_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum3), value| {
                let centered_value = *value - sample_mean;
                (
                    running_sum2 + centered_value * centered_value,
                    running_sum3 + centered_value * centered_value * centered_value,
                )
            });
    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_skewness =
        (sum_cubed_deviation / sample_count as f64) / variance_population.powf(1.5);
    if !unadjusted_skewness.is_finite() {
        return None;
    }

    let adjustment =
        ((sample_count * (sample_count - 1)) as f64).sqrt() / (sample_count - 2) as f64;
    Some(adjustment * unadjusted_skewness)
}

fn sample_adjusted_excess_kurtosis(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let sample_count = finite_values.len();
    if sample_count < 4 {
        return None;
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let (sum_squared_deviation, sum_fourth_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum4), value| {
                let centered_value = *value - sample_mean;
                let centered_square = centered_value * centered_value;
                (
                    running_sum2 + centered_square,
                    running_sum4 + centered_square * centered_square,
                )
            });
    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_excess_kurtosis = (sum_fourth_deviation / sample_count as f64)
        / (variance_population * variance_population)
        - 3.0;
    if !unadjusted_excess_kurtosis.is_finite() {
        return None;
    }

    let n = sample_count as f64;
    Some(((n - 1.0) / ((n - 2.0) * (n - 3.0))) * ((n + 1.0) * unadjusted_excess_kurtosis + 6.0))
}

fn round_positive_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = round_candidate_value(value.max(0.0));
    if value > 0.0 && rounded <= 0.0 {
        RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
    } else {
        rounded
    }
}

fn round_signed_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    (value * 1_000_000.0).round() / 1_000_000.0
}

fn round_negative_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return -0.05;
    }

    ((value.min(-1.0e-6)) * 1_000_000.0).round() / 1_000_000.0
}

fn unique_sorted_signed(values: &[f64]) -> Vec<f64> {
    let mut retained_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    retained_values.sort_by(f64::total_cmp);
    retained_values.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-12);
    retained_values
}

fn arithmetic_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let sum: f64 = values.iter().copied().sum();
    if !sum.is_finite() {
        return 0.0;
    }

    (sum / values.len() as f64).max(0.0)
}

fn deterministic_quantile_bins(values: &[f64], quantiles: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return vec![0.0];
    }

    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    sorted_values.sort_by(f64::total_cmp);

    if sorted_values.is_empty() {
        return vec![0.0];
    }

    let mut bins = Vec::with_capacity(quantiles.len());
    for quantile in quantiles {
        let clamped_quantile = quantile.clamp(0.0, 1.0);
        let quantile_position =
            (clamped_quantile * (sorted_values.len().saturating_sub(1)) as f64).round() as usize;
        bins.push(round_candidate_value(sorted_values[quantile_position]));
    }

    deduplicate_sorted_f64_values(bins)
}

fn round_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    if rounded < 0.0 { 0.0 } else { rounded }
}

fn deduplicate_sorted_f64_values(mut values: Vec<f64>) -> Vec<f64> {
    values.retain(|value| value.is_finite() && *value >= 0.0);
    values.sort_by(f64::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= 1e-12);
    values
}

fn collect_return_components_stage_constraint_violations(
    candidate_input: &ReturnComponentsStageCandidateInput,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = Vec::new();

    if !candidate_input
        .nig_shared_plus_shift_configuration
        .delta_beta
        .is_finite()
        || candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta
            >= 0.0
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
            message:
                "delta_beta must be finite and strictly negative to preserve sell-side skew asymmetry"
                    .to_string(),
        });
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        candidate_input.nig_shared_plus_shift_configuration.alpha,
        candidate_input.nig_shared_plus_shift_configuration.beta_0,
        candidate_input.nig_shared_plus_shift_configuration.delta,
        candidate_input.nig_shared_plus_shift_configuration.mu,
        candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if let Err(source) = NewsProcessConfiguration::try_new(
        candidate_input.news_process_configuration.arrivals.clone(),
        candidate_input.news_process_configuration.shared_mark_law,
        candidate_input
            .news_process_configuration
            .known_component_mark_scales,
        candidate_input
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "news_process_configuration".to_string(),
            message: source.to_string(),
        });
    }

    violations
}

fn return_components_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![CalibrationOptimizerParameterBound {
        parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
        lower_inclusive: -f64::MAX,
        upper_inclusive: 0.0,
    }]
}

fn build_return_components_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<ReturnComponentsStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::ReturnComponents)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => {
                ReturnComponentsStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: true,
                    artifact_reference: None,
                }
            }
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                ReturnComponentsStageHandoffOutput {
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

fn build_return_components_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ReturnComponentsStageExecutionFailure,
) -> ReturnComponentsStageExecutionReport {
    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::calendar::ExchangeCalendarRegistryLoader;

    #[test]
    fn derived_close_open_gap_detection_filters_zero_gap_boundaries() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0,
                post_shock_activity_lift: 1.25,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("zero-gap boundary should validate");

        assert!(derived_candidates.is_empty());
    }

    #[test]
    fn derived_close_open_gap_detection_emits_overnight_gap_candidate() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0 * (-0.013_f64).exp(),
                post_shock_activity_lift: 1.40,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("overnight boundary should validate");

        assert_eq!(derived_candidates.len(), 1);
        assert_eq!(
            derived_candidates[0].close_timestamp_utc_nanos,
            utc_nanos(2026, 2, 2, 21, 0, 0)
        );
        assert_eq!(
            derived_candidates[0].open_timestamp_utc_nanos,
            utc_nanos(2026, 2, 3, 14, 30, 0)
        );
        assert!((derived_candidates[0].gap_return + 0.013).abs() <= 1e-12);
        assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.40);
    }

    #[test]
    fn derived_close_open_gap_detection_emits_weekend_gap_candidate() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 6, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 9, 14, 30, 0),
                open_price: 100.0 * 0.012_f64.exp(),
                post_shock_activity_lift: 1.15,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("weekend boundary should validate");

        assert_eq!(derived_candidates.len(), 1);
        assert_eq!(
            derived_candidates[0].close_timestamp_utc_nanos,
            utc_nanos(2026, 2, 6, 21, 0, 0)
        );
        assert_eq!(
            derived_candidates[0].open_timestamp_utc_nanos,
            utc_nanos(2026, 2, 9, 14, 30, 0)
        );
        assert!((derived_candidates[0].gap_return - 0.012).abs() <= 1e-12);
        assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.15);
    }

    #[test]
    fn derived_close_open_gap_detection_rejects_non_boundary_timestamps() {
        let calendar_registry = test_calendar_registry();

        let error = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 20, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0 * (-0.013_f64).exp(),
                post_shock_activity_lift: 1.40,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect_err("non-boundary close timestamp must fail");

        assert!(matches!(
            error,
            ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition { .. }
        ));
    }

    #[test]
    fn nig_search_candidates_remain_non_empty_for_tiny_fx_return_volatility() {
        let summary_statistics = ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: 3,
            known_realized_sample_count: 1,
            source_window_duration_hours: 12.0,
            observed_open_session_return_count: 2_000,
            continuous_fit_return_count: 1_980,
            intraday_jump_candidate_count: 0,
            close_open_gap_candidate_count: 0,
            open_session_sell_share_proxy: 0.51,
            mean_continuous_business_time_scale: 1.0,
            continuous_return_mean: 0.0,
            continuous_return_volatility: 2.5e-6,
            continuous_return_skewness: -0.15,
            continuous_return_excess_kurtosis: 1.2,
            observed_log_price_span: 0.012,
            mean_abs_known_open_jump_return: 0.0008,
            mean_abs_known_closed_gap_return: 0.0,
            mean_abs_surprise_open_jump_return: 0.0,
            mean_abs_surprise_closed_gap_return: 0.0,
            mean_known_post_shock_activity_lift: 1.05,
            mean_surprise_post_shock_activity_lift: 0.0,
            known_news_coverage_ratio: 1.0,
            surprise_arrival_intensity_per_hour: 0.0,
        };

        let nig_candidates = build_nig_search_candidates(summary_statistics);

        assert!(!nig_candidates.is_empty());
        assert!(nig_candidates.iter().all(|candidate| candidate.delta
            >= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER));
    }

    fn test_source_window() -> ReturnComponentsStageSignalSourceWindow {
        ReturnComponentsStageSignalSourceWindow {
            window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
            window_end_utc_nanos: utc_nanos(2026, 2, 10, 0, 0, 0),
        }
    }

    fn test_calendar_registry() -> ExchangeCalendarRegistry {
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
        {"weekday": "Monday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Tuesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Wednesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Thursday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Friday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]}
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

    fn utc_nanos(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("fixture timestamp should be valid")
            .timestamp_nanos_opt()
            .expect("fixture timestamp nanos should fit in i64")
    }
}
