use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use crate::nig::NigSharedPlusShiftConfiguration;

use super::super::contracts::{
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use super::super::objective::{HawkesStageObjectiveTermInput, HawkesStageObjectiveTermInputError};
use super::super::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunMetadata,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use super::hawkes::{HawkesStageFittedPayload, HawkesStageObjectiveTermDiagnostic};
use super::seasonality::CalibrationStageExecutionStatus;

const RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY: &str =
    "known_calendar_and_observed_jump_gap_signals_to_candidate_families.v1";
const RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY: &str =
    "timestamp_sorted_finite_signal_filtering_with_deterministic_quantile_binning.v1";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY: &str =
    "open_jump_closed_gap_activity_coupling_bucketed_weighted_terms.v1";
const RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY: &str =
    "stage_seed_reused_for_candidate_generation_enumeration.v1";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT: &str = "open_jump_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT: &str = "closed_gap_fit";
const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT: &str = "activity_coupling_fit";
const RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_KNOWN_RETENTION_QUANTILES: [f64; 2] = [0.0, 0.5];

/// One weighted objective term used by return-components-stage candidate evaluation.
pub type ReturnComponentsStageObjectiveTermInput = HawkesStageObjectiveTermInput;

/// One weighted objective-term diagnostic emitted by return-components-stage evaluation.
pub type ReturnComponentsStageObjectiveTermDiagnostic = HawkesStageObjectiveTermDiagnostic;

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

/// Objective metric-bucket weights for candidate generation and evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnComponentsStageObjectiveMetricWeights {
    pub open_jump_fit_weight: f64,
    pub closed_gap_fit_weight: f64,
    pub activity_coupling_fit_weight: f64,
}

impl ReturnComponentsStageObjectiveMetricWeights {
    pub const fn new(
        open_jump_fit_weight: f64,
        closed_gap_fit_weight: f64,
        activity_coupling_fit_weight: f64,
    ) -> Self {
        Self {
            open_jump_fit_weight,
            closed_gap_fit_weight,
            activity_coupling_fit_weight,
        }
    }

    fn validate(self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        if !self.open_jump_fit_weight.is_finite() || self.open_jump_fit_weight < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidOpenJumpFitWeight {
                    value: self.open_jump_fit_weight,
                },
            );
        }
        if !self.closed_gap_fit_weight.is_finite() || self.closed_gap_fit_weight < 0.0 {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::InvalidClosedGapFitWeight {
                    value: self.closed_gap_fit_weight,
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

        Ok(())
    }
}

/// Canonical candidate-generation payload derived from observed return and activity signals.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageCandidateGenerationInput {
    pub known_news_calendar: ReturnComponentsKnownNewsCalendarInput,
    pub realized_known_news_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    pub detected_intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    pub detected_close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    pub nig_shared_plus_shift_candidates: Vec<NigSharedPlusShiftConfiguration>,
    pub shared_mark_law_template: SharedNewsMarkLawConfiguration,
    pub objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
}

impl ReturnComponentsStageCandidateGenerationInput {
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
        self.known_news_calendar.validate()?;

        if self.realized_known_news_return_samples.is_empty() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::MissingKnownNewsReturnSamples,
            );
        }
        if self.detected_intraday_jump_candidates.is_empty() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::MissingIntradayJumpCandidates,
            );
        }
        if self.detected_close_open_gap_candidates.is_empty() {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::MissingCloseOpenGapCandidates,
            );
        }
        if self.nig_shared_plus_shift_candidates.is_empty() {
            return Err(ReturnComponentsStageCandidateGenerationInputError::MissingNigCandidates);
        }

        for sample in &self.realized_known_news_return_samples {
            sample.validate(self.known_news_calendar.source_window)?;
        }
        for jump_candidate in &self.detected_intraday_jump_candidates {
            jump_candidate.validate(self.known_news_calendar.source_window)?;
        }
        for gap_candidate in &self.detected_close_open_gap_candidates {
            gap_candidate.validate(self.known_news_calendar.source_window)?;
        }

        for (candidate_position, nig_candidate) in
            self.nig_shared_plus_shift_candidates.iter().enumerate()
        {
            if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
                nig_candidate.alpha,
                nig_candidate.beta_0,
                nig_candidate.delta,
                nig_candidate.mu,
                nig_candidate.delta_beta,
            ) {
                return Err(
                    ReturnComponentsStageCandidateGenerationInputError::InvalidNigCandidate {
                        candidate_position,
                        error_message: source.to_string(),
                    },
                );
            }
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
    MissingKnownNewsReturnSamples,
    MissingIntradayJumpCandidates,
    MissingCloseOpenGapCandidates,
    MissingNigCandidates,
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
    InvalidNigCandidate {
        candidate_position: usize,
        error_message: String,
    },
    InvalidSharedMarkLawTemplate {
        error_message: String,
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
            Self::MissingKnownNewsReturnSamples => write!(
                formatter,
                "candidate generation requires at least one realized_known_news_return_sample"
            ),
            Self::MissingIntradayJumpCandidates => write!(
                formatter,
                "candidate generation requires at least one detected_intraday_jump_candidate"
            ),
            Self::MissingCloseOpenGapCandidates => write!(
                formatter,
                "candidate generation requires at least one detected_close_open_gap_candidate"
            ),
            Self::MissingNigCandidates => write!(
                formatter,
                "candidate generation requires at least one nig_shared_plus_shift_candidate"
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
            Self::InvalidNigCandidate {
                candidate_position,
                error_message,
            } => write!(
                formatter,
                "nig_shared_plus_shift_candidates[{candidate_position}] is invalid: {error_message}"
            ),
            Self::InvalidSharedMarkLawTemplate { error_message } => write!(
                formatter,
                "shared_mark_law_template is invalid for candidate generation: {error_message}"
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
    pub known_return_jump_scale: f64,
    pub surprise_return_jump_scale: f64,
    pub sigma_g: f64,
}

impl ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_scale: self.known_return_jump_scale,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidReturnJumpMarkScaleFamily {
                error_message: source.to_string(),
            }
        })?;

        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_scale: self.surprise_return_jump_scale,
        }
        .validate()
        .map_err(|source| {
            ReturnComponentsStageCandidateInputError::InvalidReturnJumpMarkScaleFamily {
                error_message: source.to_string(),
            }
        })?;

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
    pub intraday_jump_candidate_count: usize,
    pub close_open_gap_candidate_count: usize,
    pub mean_abs_open_jump_return: f64,
    pub mean_abs_closed_gap_return: f64,
    pub mean_post_shock_activity_lift: f64,
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
            summary_statistics.mean_abs_open_jump_return,
            summary_statistics.mean_abs_closed_gap_return,
            summary_statistics.mean_post_shock_activity_lift,
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
    pub open_jump_fit: f64,
    pub closed_gap_fit: f64,
    pub activity_coupling_fit: f64,
}

impl ReturnComponentsStageObjectiveMetricBuckets {
    fn validate(self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        for metric_value in [
            self.open_jump_fit,
            self.closed_gap_fit,
            self.activity_coupling_fit,
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

        let mut has_open_jump_term = false;
        let mut has_closed_gap_term = false;
        let mut has_activity_coupling_term = false;

        for objective_term in &self.objective_terms {
            objective_term.validate().map_err(|source| {
                ReturnComponentsStageCandidateInputError::InvalidObjectiveTerm {
                    term_id: objective_term.term_id.clone(),
                    source,
                }
            })?;

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

        Ok(())
    }
}

/// Upstream activity-side handoff input resolved for return-components-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageActivityInput {
    InMemoryOutput { payload: HawkesStageFittedPayload },
    PersistedArtifactReference { artifact_reference: String },
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
            Self::PersistedArtifactReference { artifact_reference } => Some(artifact_reference),
        }
    }

    fn selected_hawkes_candidate_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.selected_candidate_id.as_str()),
            Self::PersistedArtifactReference { .. } => None,
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
        Self {
            activity_input: ReturnComponentsStageActivityInput::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageFittedParameters {
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
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
        &stage_input.candidate_generation_input,
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
        activity_selected_hawkes_candidate_id: stage_input
            .activity_input
            .selected_hawkes_candidate_id()
            .map(ToOwned::to_owned),
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
    stage_seed: u64,
) -> Result<
    ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    candidate_generation_input.validate()?;

    let sorted_signals = sort_candidate_generation_signals(candidate_generation_input);
    let summary_statistics =
        summarize_candidate_generation_signals(candidate_generation_input, &sorted_signals);

    let jump_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.open_jump_magnitudes.as_slice(),
        &RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES,
    );
    let gap_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.closed_gap_magnitudes.as_slice(),
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
    );
    let candidate_family_counts =
        build_candidate_family_counts(candidate_generation_input, &candidate_family_sets);
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

#[derive(Debug, Clone)]
struct ReturnComponentsStageSortedSignalInputs {
    known_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    known_sample_magnitudes: Vec<f64>,
    open_jump_magnitudes: Vec<f64>,
    closed_gap_magnitudes: Vec<f64>,
    activity_lifts: Vec<f64>,
    source_window: ReturnComponentsStageSignalSourceWindow,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStageCandidateFamilySets {
    known_arrival_families: Vec<ReturnComponentsKnownNewsArrivalCandidateFamily>,
    surprise_arrival_families: Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily>,
    return_jump_mark_scale_families: Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily>,
    activity_coupling_families: Vec<ReturnComponentsActivityCouplingCandidateFamily>,
}

fn sort_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
) -> ReturnComponentsStageSortedSignalInputs {
    let mut known_return_samples = candidate_generation_input
        .realized_known_news_return_samples
        .clone();
    known_return_samples.sort_by_key(|sample| sample.event_timestamp_utc_nanos);

    let mut intraday_jump_candidates = candidate_generation_input
        .detected_intraday_jump_candidates
        .clone();
    intraday_jump_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);

    let mut close_open_gap_candidates = candidate_generation_input
        .detected_close_open_gap_candidates
        .clone();
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

    let open_jump_magnitudes = {
        let mut magnitudes =
            Vec::with_capacity(known_return_samples.len() + intraday_jump_candidates.len());
        magnitudes.extend(
            known_return_samples
                .iter()
                .map(|sample| sample.open_window_jump_return.abs()),
        );
        magnitudes.extend(
            intraday_jump_candidates
                .iter()
                .map(|candidate| candidate.jump_return.abs()),
        );
        magnitudes
    };

    let closed_gap_magnitudes = {
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

    let activity_lifts = {
        let mut lifts =
            Vec::with_capacity(intraday_jump_candidates.len() + close_open_gap_candidates.len());
        lifts.extend(
            intraday_jump_candidates
                .iter()
                .map(|candidate| candidate.post_shock_activity_lift),
        );
        lifts.extend(
            close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.post_shock_activity_lift),
        );
        lifts
    };

    ReturnComponentsStageSortedSignalInputs {
        known_return_samples,
        intraday_jump_candidates,
        close_open_gap_candidates,
        known_sample_magnitudes,
        open_jump_magnitudes,
        closed_gap_magnitudes,
        activity_lifts,
        source_window: candidate_generation_input.known_news_calendar.source_window,
    }
}

fn summarize_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
) -> ReturnComponentsStageObservedSignalSummaryStatistics {
    let source_window_duration_hours = sorted_signals
        .source_window
        .duration_hours()
        .max(f64::EPSILON);

    ReturnComponentsStageObservedSignalSummaryStatistics {
        known_calendar_event_count: candidate_generation_input
            .known_news_calendar
            .scheduled_event_timestamps_utc_nanos
            .len(),
        known_realized_sample_count: sorted_signals.known_return_samples.len(),
        intraday_jump_candidate_count: sorted_signals.intraday_jump_candidates.len(),
        close_open_gap_candidate_count: sorted_signals.close_open_gap_candidates.len(),
        mean_abs_open_jump_return: arithmetic_mean(sorted_signals.open_jump_magnitudes.as_slice()),
        mean_abs_closed_gap_return: arithmetic_mean(
            sorted_signals.closed_gap_magnitudes.as_slice(),
        ),
        mean_post_shock_activity_lift: arithmetic_mean(sorted_signals.activity_lifts.as_slice()),
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
    let known_arrival_families = build_known_arrival_families(
        candidate_generation_input,
        sorted_signals.known_sample_magnitudes.as_slice(),
        sorted_signals.known_return_samples.as_slice(),
    );
    let surprise_arrival_families =
        build_surprise_arrival_families(summary_statistics.surprise_arrival_intensity_per_hour);
    let return_jump_mark_scale_families = build_return_jump_mark_scale_families(
        candidate_generation_input,
        summary_statistics.mean_abs_open_jump_return,
        jump_abs_return_bin_edges,
        gap_abs_return_bin_edges,
    );
    let activity_coupling_families =
        build_activity_coupling_families(summary_statistics.mean_post_shock_activity_lift);

    ReturnComponentsStageCandidateFamilySets {
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
) -> Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily> {
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
    mean_abs_open_jump_return: f64,
    jump_abs_return_bin_edges: &[f64],
    gap_abs_return_bin_edges: &[f64],
) -> Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily> {
    let open_scale_target = mean_abs_open_jump_return.max(1e-8);
    let mut return_jump_scale_values: Vec<f64> = jump_abs_return_bin_edges
        .iter()
        .copied()
        .map(|value| round_candidate_value(value / open_scale_target))
        .collect();
    if return_jump_scale_values.is_empty() {
        return_jump_scale_values.push(1.0);
    }
    return_jump_scale_values = deduplicate_sorted_f64_values(return_jump_scale_values);

    let mut sigma_g_values: Vec<f64> = gap_abs_return_bin_edges
        .iter()
        .copied()
        .map(round_candidate_value)
        .collect();
    if sigma_g_values.is_empty() {
        sigma_g_values.push(round_candidate_value(
            candidate_generation_input
                .shared_mark_law_template
                .coupled_gap_marks
                .sigma_g,
        ));
    }
    sigma_g_values = deduplicate_sorted_f64_values(sigma_g_values);

    let mut return_jump_mark_scale_families = Vec::new();
    for return_jump_scale in &return_jump_scale_values {
        for sigma_g in &sigma_g_values {
            return_jump_mark_scale_families.push(
                ReturnComponentsReturnJumpMarkScaleCandidateFamily {
                    known_return_jump_scale: *return_jump_scale,
                    surprise_return_jump_scale: *return_jump_scale,
                    sigma_g: *sigma_g,
                },
            );
        }
    }

    return_jump_mark_scale_families
}

fn build_activity_coupling_families(
    mean_post_shock_activity_lift: f64,
) -> Vec<ReturnComponentsActivityCouplingCandidateFamily> {
    let mut activity_scale_values = deduplicate_sorted_f64_values(vec![
        round_candidate_value(mean_post_shock_activity_lift * 0.75),
        round_candidate_value(mean_post_shock_activity_lift),
        round_candidate_value(mean_post_shock_activity_lift * 1.25),
    ]);
    if activity_scale_values.is_empty() {
        activity_scale_values.push(0.0);
    }

    activity_scale_values
        .into_iter()
        .map(
            |coupling_scale| ReturnComponentsActivityCouplingCandidateFamily {
                known_activity_coupling_scale: coupling_scale,
                surprise_activity_coupling_scale: coupling_scale,
            },
        )
        .collect()
}

fn build_candidate_provenance_template(
    source_window: ReturnComponentsStageSignalSourceWindow,
    stage_seed: u64,
    jump_abs_return_bin_edges: Vec<f64>,
    gap_abs_return_bin_edges: Vec<f64>,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
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
                source_name: "detected_intraday_jump_candidates".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "detected_close_open_gap_candidates".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
        ],
        preprocessing_filter_policy: RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
        preprocessing_filters: vec![
            "reject_non_finite_known_news_returns".to_string(),
            "reject_non_finite_intraday_jump_returns".to_string(),
            "reject_non_finite_close_open_gap_returns".to_string(),
            "enforce_non_negative_post_shock_activity_lift".to_string(),
            "sort_intraday_jumps_by_jump_timestamp".to_string(),
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
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
) -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: candidate_generation_input
            .nig_shared_plus_shift_candidates
            .len(),
        known_news_arrival_family_count: candidate_family_sets.known_arrival_families.len(),
        surprise_news_arrival_family_count: candidate_family_sets.surprise_arrival_families.len(),
        return_jump_mark_scale_family_count: candidate_family_sets
            .return_jump_mark_scale_families
            .len(),
        activity_coupling_family_count: candidate_family_sets.activity_coupling_families.len(),
    }
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

    for (nig_position, nig_candidate) in candidate_generation_input
        .nig_shared_plus_shift_candidates
        .iter()
        .enumerate()
    {
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

                        let objective_metric_buckets = score_candidate_metric_buckets(
                            candidate_families.clone(),
                            summary_statistics,
                        );
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
                            nig_shared_plus_shift_configuration: nig_candidate.clone(),
                            news_process_configuration,
                            objective_metric_buckets,
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
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT.to_string(),
            weight: objective_metric_weights.open_jump_fit_weight,
            observed_score: objective_metric_buckets.open_jump_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT.to_string(),
            weight: objective_metric_weights.closed_gap_fit_weight,
            observed_score: objective_metric_buckets.closed_gap_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT.to_string(),
            weight: objective_metric_weights.activity_coupling_fit_weight,
            observed_score: objective_metric_buckets.activity_coupling_fit,
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
            return_jump_scale: return_jump_family.known_return_jump_scale,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.surprise_activity_coupling_scale,
            return_jump_scale: return_jump_family.surprise_return_jump_scale,
        },
    )
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
            error_message: source.to_string(),
        }
    })
}

fn score_candidate_metric_buckets(
    candidate_families: ReturnComponentsStageCandidateFamilies,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> ReturnComponentsStageObjectiveMetricBuckets {
    let known_retention_ratio = candidate_families
        .known_news_arrival_controls
        .retained_known_event_timestamps_utc_nanos
        .len() as f64
        / summary_statistics.known_calendar_event_count.max(1) as f64;

    let known_arrival_fit_error = relative_abs_error(
        known_retention_ratio,
        summary_statistics.known_news_coverage_ratio,
    );

    let surprise_arrival_fit_error = relative_abs_error(
        candidate_families
            .surprise_news_arrival_controls
            .baseline_intensity,
        summary_statistics.surprise_arrival_intensity_per_hour,
    );

    let return_jump_scale_target = summary_statistics.mean_abs_open_jump_return.max(1e-8);
    let average_return_jump_scale = (candidate_families
        .return_jump_mark_scale_controls
        .known_return_jump_scale
        + candidate_families
            .return_jump_mark_scale_controls
            .surprise_return_jump_scale)
        / 2.0;

    let open_jump_fit = relative_abs_error(average_return_jump_scale, return_jump_scale_target)
        + 0.25 * known_arrival_fit_error
        + 0.25 * surprise_arrival_fit_error;

    let closed_gap_fit = relative_abs_error(
        candidate_families.return_jump_mark_scale_controls.sigma_g,
        summary_statistics.mean_abs_closed_gap_return.max(1e-8),
    ) + 0.5 * known_arrival_fit_error;

    let activity_coupling_target = summary_statistics.mean_post_shock_activity_lift.max(1e-8);
    let average_activity_coupling_scale = (candidate_families
        .news_to_hawkes_activity_coupling_controls
        .known_activity_coupling_scale
        + candidate_families
            .news_to_hawkes_activity_coupling_controls
            .surprise_activity_coupling_scale)
        / 2.0;
    let activity_coupling_fit =
        relative_abs_error(average_activity_coupling_scale, activity_coupling_target)
            + 0.25 * surprise_arrival_fit_error;

    ReturnComponentsStageObjectiveMetricBuckets {
        open_jump_fit: round_candidate_value(open_jump_fit),
        closed_gap_fit: round_candidate_value(closed_gap_fit),
        activity_coupling_fit: round_candidate_value(activity_coupling_fit),
    }
}

fn relative_abs_error(observed: f64, target: f64) -> f64 {
    let denominator = target.abs().max(1e-8);
    ((observed - target).abs() / denominator).max(0.0)
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
