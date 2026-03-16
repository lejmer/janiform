//! Gap-behavior diagnostics covering open-window direct jumps,
//! closed-window gaps, and open-versus-closed contrast behavior.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use super::diagnostics_shared::{
    BlockBootstrapConfiguration, build_delta_uncertainty_context,
    compute_block_bootstrap_confidence_interval, compute_sorted_quantile, coverage_ratio,
    derive_metric_bootstrap_seed, validate_block_bootstrap_configuration,
};
use super::metric_catalog::{
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE, StylizedFactMetricDomain,
    StylizedFactMetricFamily, StylizedFactMetricHorizonApplicability,
    build_stylized_fact_metric_catalog,
};
use super::time_buckets::{
    TimestampAnalysisWindow, interval_count_for_window, interval_position_for_timestamp,
};
use serde::Serialize;

const OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID: &str =
    "open_jump_abs_tail_expected_shortfall_ratio";
const CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID: &str =
    "closed_gap_abs_tail_expected_shortfall_ratio";
const CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID: &str =
    "closed_gap_vs_open_jump_wasserstein_distance";
const TAIL_QUANTILE_LEVEL: f64 = 0.95;
const MINIMUM_DENOMINATOR_MAGNITUDE: f64 = 1e-12;
const ROUTING_TOLERANCE: f64 = 1e-12;

pub const GAP_BEHAVIOR_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str =
    "validation.stylized_fact.gap_behavior_open_closed_contrast.diagnostics.v1";
pub const GAP_BEHAVIOR_DIAGNOSTICS_DETERMINISM_POLICY: &str =
    "stable_horizon_and_scenario_input_order_with_seeded_block_bootstrap.v1";
pub const GAP_BEHAVIOR_DIAGNOSTICS_BOOTSTRAP_POLICY: &str =
    "moving_block_bootstrap_with_wraparound_and_percentile_intervals.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NewsSemanticsScenarioClass {
    KnownOnly,
    SurpriseOnly,
    MixedNews,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapDeterministicReplayMetadata {
    pub replay_baseline_id: String,
    pub event_calendar_binding_id: String,
    pub jump_observation_binding_id: String,
    pub gap_observation_binding_id: String,
    pub candidate_selection_policy_id: String,
    pub candidate_selection_rank: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapIssueTraceabilityMetadata {
    pub issue_id: String,
    pub semantic_focus_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GapBehaviorDiagnosticsRequest {
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub empirical_dataset: GapBehaviorDatasetInput,
    pub simulated_dataset: GapBehaviorDatasetInput,
    pub horizon_buckets: Vec<GapBehaviorHorizonBucket>,
    pub bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
    pub sample_sufficiency_policy: GapBehaviorSampleSufficiencyPolicy,
    pub threshold_policy: GapBehaviorThresholdPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GapBehaviorDatasetInput {
    pub dataset_id: String,
    pub scenarios: Vec<GapBehaviorScenarioInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GapBehaviorScenarioInput {
    pub scenario_id: String,
    pub exchange_code: String,
    pub scenario_class: NewsSemanticsScenarioClass,
    pub deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata,
    pub issue_traceability: OpenClosedGapIssueTraceabilityMetadata,
    pub tags: Vec<String>,
    pub event_observations: Vec<GapBehaviorEventObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GapBehaviorMarketState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GapBehaviorClosedWindowRegime {
    Overnight,
    Weekend,
    OtherClosed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GapBehaviorEventObservation {
    pub event_id: String,
    pub timestamp_utc_nanos: i64,
    pub market_state: GapBehaviorMarketState,
    pub closed_window_regime: Option<GapBehaviorClosedWindowRegime>,
    pub activity_coupling_increment: f64,
    pub open_jump_increment: f64,
    pub closed_gap_increment: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapBehaviorHorizonBucket {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapBehaviorBlockBootstrapConfiguration {
    pub confidence_level: f64,
    pub replication_count: usize,
    pub block_length_interval_count: usize,
    pub random_seed: u64,
}

impl Default for GapBehaviorBlockBootstrapConfiguration {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            replication_count: 2_000,
            block_length_interval_count: 5,
            random_seed: 5_600_091,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorMetricSampleSufficiencyPolicy {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio: f64,
}

impl GapBehaviorMetricSampleSufficiencyPolicy {
    fn validate(self, field_prefix: &'static str) -> Result<(), GapBehaviorDiagnosticsError> {
        if self.minimum_sample_count == 0 {
            return Err(
                GapBehaviorDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_sample_count"),
                    details: "minimum_sample_count must be greater than 0".to_string(),
                },
            );
        }

        if self.minimum_effective_sample_count == 0
            || self.minimum_effective_sample_count > self.minimum_sample_count
        {
            return Err(
                GapBehaviorDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_effective_sample_count"),
                    details: format!(
                        "minimum_effective_sample_count must be in [1, minimum_sample_count], found {}",
                        self.minimum_effective_sample_count
                    ),
                },
            );
        }

        if self.minimum_tail_observation_count > self.minimum_effective_sample_count {
            return Err(
                GapBehaviorDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_tail_observation_count"),
                    details: format!(
                        "minimum_tail_observation_count must be less than or equal to minimum_effective_sample_count, found {}",
                        self.minimum_tail_observation_count
                    ),
                },
            );
        }

        if !self.minimum_horizon_coverage_ratio.is_finite()
            || self.minimum_horizon_coverage_ratio <= 0.0
            || self.minimum_horizon_coverage_ratio > 1.0
        {
            return Err(
                GapBehaviorDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_horizon_coverage_ratio"),
                    details: format!(
                        "minimum_horizon_coverage_ratio must be finite and in (0, 1], found {}",
                        self.minimum_horizon_coverage_ratio
                    ),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorSampleSufficiencyPolicy {
    pub open_jump_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy,
    pub closed_gap_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy,
    pub closed_gap_vs_open_jump_wasserstein_distance: GapBehaviorMetricSampleSufficiencyPolicy,
}

impl Default for GapBehaviorSampleSufficiencyPolicy {
    fn default() -> Self {
        Self {
            open_jump_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy {
                minimum_sample_count: 180,
                minimum_effective_sample_count: 120,
                minimum_tail_observation_count: 25,
                minimum_horizon_coverage_ratio: 0.7,
            },
            closed_gap_abs_tail_expected_shortfall_ratio:
                GapBehaviorMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 120,
                    minimum_effective_sample_count: 90,
                    minimum_tail_observation_count: 20,
                    minimum_horizon_coverage_ratio: 0.6,
                },
            closed_gap_vs_open_jump_wasserstein_distance:
                GapBehaviorMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 200,
                    minimum_effective_sample_count: 140,
                    minimum_tail_observation_count: 0,
                    minimum_horizon_coverage_ratio: 0.65,
                },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GapBehaviorThresholdPolicy {
    pub comparison_policy_id: String,
    pub open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta: f64,
    pub closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta: f64,
    pub closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta: f64,
}

impl Default for GapBehaviorThresholdPolicy {
    fn default() -> Self {
        Self {
            comparison_policy_id: "absolute_delta_threshold_policy.v1".to_string(),
            open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta: 0.35,
            closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta: 0.35,
            closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta: 0.25,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorDiagnosticsReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub bootstrap_policy: &'static str,
    pub insufficient_data_status_code: &'static str,
    pub metric_catalog_id: String,
    pub metric_catalog_version: String,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub empirical_scenarios: Vec<GapBehaviorScenarioMetadata>,
    pub simulated_scenarios: Vec<GapBehaviorScenarioMetadata>,
    pub horizons: Vec<GapBehaviorHorizonDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorScenarioMetadata {
    pub scenario_id: String,
    pub exchange_code: String,
    pub scenario_class: NewsSemanticsScenarioClass,
    pub deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata,
    pub issue_traceability: OpenClosedGapIssueTraceabilityMetadata,
    pub tags: Vec<String>,
    pub total_event_count: usize,
    pub in_window_event_count: usize,
    pub analysis_window_coverage_ratio: f64,
    pub open_window_event_count: usize,
    pub closed_window_event_count: usize,
    pub overnight_closed_window_event_count: usize,
    pub weekend_closed_window_event_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorHorizonDiagnostics {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
    pub component_coupling_summary: GapBehaviorComponentCouplingSummaryComparison,
    pub open_jump_abs_tail_expected_shortfall_ratio: GapBehaviorMetricComparison,
    pub closed_gap_abs_tail_expected_shortfall_ratio: GapBehaviorMetricComparison,
    pub closed_gap_vs_open_jump_wasserstein_distance: GapBehaviorMetricComparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorComponentCouplingSummary {
    pub open_window_event_count: usize,
    pub closed_window_event_count: usize,
    pub overnight_closed_window_event_count: usize,
    pub weekend_closed_window_event_count: usize,
    pub other_closed_window_event_count: usize,
    pub activity_coupling_total: f64,
    pub open_jump_total: f64,
    pub closed_gap_total: f64,
    pub overnight_closed_gap_total: f64,
    pub weekend_closed_gap_total: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorComponentCouplingSummaryComparison {
    pub empirical: GapBehaviorComponentCouplingSummary,
    pub simulated: GapBehaviorComponentCouplingSummary,
    pub activity_coupling_absolute_delta: f64,
    pub open_jump_absolute_delta: f64,
    pub closed_gap_absolute_delta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapBehaviorMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GapBehaviorSampleCoverage {
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GapBehaviorObservedCounts {
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorDatasetMetricResult {
    pub status: GapBehaviorMetricStatus,
    pub value: Option<f64>,
    pub confidence_interval: Option<GapBehaviorConfidenceInterval>,
    pub sample_coverage: GapBehaviorSampleCoverage,
    pub observed_counts: GapBehaviorObservedCounts,
    pub required_minimums: GapBehaviorMetricSampleSufficiencyPolicy,
    pub horizon_coverage_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GapBehaviorDeltaUncertaintyContext {
    pub empirical_interval_half_width: Option<f64>,
    pub simulated_interval_half_width: Option<f64>,
    pub pooled_interval_half_width: Option<f64>,
    pub standardized_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorThresholdContext {
    pub comparison_policy_id: String,
    pub max_absolute_delta: f64,
    pub absolute_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapBehaviorMetricComparison {
    pub metric_id: String,
    pub metric_name: String,
    pub status: GapBehaviorMetricStatus,
    pub empirical: GapBehaviorDatasetMetricResult,
    pub simulated: GapBehaviorDatasetMetricResult,
    pub absolute_delta: Option<f64>,
    pub uncertainty_context: GapBehaviorDeltaUncertaintyContext,
    pub threshold_context: GapBehaviorThresholdContext,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GapBehaviorDiagnosticsError {
    InvalidAnalysisWindow {
        analysis_start_utc_nanos: i64,
        analysis_end_utc_nanos: i64,
    },
    EmptyDatasetId {
        dataset_role: &'static str,
    },
    EmptyScenarioSet {
        dataset_role: &'static str,
    },
    EmptyScenarioId {
        dataset_role: &'static str,
        scenario_position: usize,
    },
    DuplicateScenarioId {
        dataset_role: &'static str,
        scenario_id: String,
    },
    EmptyScenarioTag {
        dataset_role: &'static str,
        scenario_id: String,
        tag_position: usize,
    },
    EmptyScenarioEventSet {
        dataset_role: &'static str,
        scenario_id: String,
    },
    EmptyEventId {
        dataset_role: &'static str,
        scenario_id: String,
        event_position: usize,
    },
    InvalidEventObservation {
        dataset_role: &'static str,
        scenario_id: String,
        event_id: String,
        field_name: &'static str,
        value: f64,
    },
    InvalidRoutingSemantics {
        dataset_role: &'static str,
        scenario_id: String,
        event_id: String,
        details: String,
    },
    EmptyHorizonBucketSet,
    EmptyHorizonLabel {
        horizon_position: usize,
    },
    DuplicateHorizonLabel {
        horizon_label: String,
    },
    InvalidHorizonDuration {
        horizon_label: String,
        horizon_duration_nanos: i64,
    },
    InvalidBootstrapConfiguration {
        field_name: &'static str,
        details: String,
    },
    InvalidSampleSufficiencyConfiguration {
        field_name: String,
        details: String,
    },
    InvalidThresholdPolicy {
        field_name: &'static str,
        details: String,
    },
    MetricCatalogMetricMissing {
        metric_id: &'static str,
    },
    MetricCatalogMetricInvalid {
        metric_id: &'static str,
        details: String,
    },
}

impl Display for GapBehaviorDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAnalysisWindow {
                analysis_start_utc_nanos,
                analysis_end_utc_nanos,
            } => write!(
                formatter,
                "analysis window must be strictly increasing, found start={} end={}",
                analysis_start_utc_nanos, analysis_end_utc_nanos
            ),
            Self::EmptyDatasetId { dataset_role } => {
                write!(formatter, "{dataset_role} dataset_id must be non-empty")
            }
            Self::EmptyScenarioSet { dataset_role } => write!(
                formatter,
                "{dataset_role} dataset must define at least one scenario"
            ),
            Self::EmptyScenarioId {
                dataset_role,
                scenario_position,
            } => write!(
                formatter,
                "{dataset_role} scenario at position {scenario_position} must define a non-empty scenario_id"
            ),
            Self::DuplicateScenarioId {
                dataset_role,
                scenario_id,
            } => write!(
                formatter,
                "{dataset_role} dataset has duplicate scenario_id `{scenario_id}`"
            ),
            Self::EmptyScenarioTag {
                dataset_role,
                scenario_id,
                tag_position,
            } => write!(
                formatter,
                "{dataset_role} scenario `{scenario_id}` has an empty tag at position {tag_position}"
            ),
            Self::EmptyScenarioEventSet {
                dataset_role,
                scenario_id,
            } => write!(
                formatter,
                "{dataset_role} scenario `{scenario_id}` must define at least one event observation"
            ),
            Self::EmptyEventId {
                dataset_role,
                scenario_id,
                event_position,
            } => write!(
                formatter,
                "{dataset_role} scenario `{scenario_id}` event at position {event_position} must define a non-empty event_id"
            ),
            Self::InvalidEventObservation {
                dataset_role,
                scenario_id,
                event_id,
                field_name,
                value,
            } => write!(
                formatter,
                "{dataset_role} scenario `{scenario_id}` event `{event_id}` has non-finite {field_name}: {value}"
            ),
            Self::InvalidRoutingSemantics {
                dataset_role,
                scenario_id,
                event_id,
                details,
            } => write!(
                formatter,
                "{dataset_role} scenario `{scenario_id}` event `{event_id}` violates open-closed routing semantics: {details}"
            ),
            Self::EmptyHorizonBucketSet => write!(
                formatter,
                "gap behavior diagnostics requires at least one horizon bucket"
            ),
            Self::EmptyHorizonLabel { horizon_position } => write!(
                formatter,
                "horizon bucket at position {horizon_position} must define a non-empty horizon_label"
            ),
            Self::DuplicateHorizonLabel { horizon_label } => {
                write!(
                    formatter,
                    "horizon_label `{horizon_label}` is duplicated in request"
                )
            }
            Self::InvalidHorizonDuration {
                horizon_label,
                horizon_duration_nanos,
            } => write!(
                formatter,
                "horizon `{horizon_label}` must have a strictly positive horizon_duration_nanos, found {horizon_duration_nanos}"
            ),
            Self::InvalidBootstrapConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "bootstrap configuration field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidSampleSufficiencyConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "sample sufficiency policy field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidThresholdPolicy {
                field_name,
                details,
            } => write!(
                formatter,
                "threshold policy field `{field_name}` is invalid: {details}"
            ),
            Self::MetricCatalogMetricMissing { metric_id } => write!(
                formatter,
                "metric catalog does not define required gap-behavior metric `{metric_id}`"
            ),
            Self::MetricCatalogMetricInvalid { metric_id, details } => write!(
                formatter,
                "metric catalog metric `{metric_id}` is invalid for gap-behavior diagnostics: {details}"
            ),
        }
    }
}

impl Error for GapBehaviorDiagnosticsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GapBehaviorMetricCatalogContext {
    catalog_id: String,
    catalog_version: String,
    open_jump_metric_name: String,
    closed_gap_metric_name: String,
    contrast_metric_name: String,
}

pub fn run_gap_behavior_diagnostics(
    request: GapBehaviorDiagnosticsRequest,
) -> Result<GapBehaviorDiagnosticsReport, GapBehaviorDiagnosticsError> {
    validate_request(&request)?;

    let metric_catalog_context = resolve_metric_catalog_context()?;

    let analysis_window = TimestampAnalysisWindow::new(
        request.analysis_start_utc_nanos,
        request.analysis_end_utc_nanos,
    );

    let empirical_dataset =
        prepare_dataset("empirical", &request.empirical_dataset, analysis_window)?;
    let simulated_dataset =
        prepare_dataset("simulated", &request.simulated_dataset, analysis_window)?;

    let mut horizons: Vec<GapBehaviorHorizonDiagnostics> =
        Vec::with_capacity(request.horizon_buckets.len());
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        let empirical_horizon_series = build_horizon_dataset_series(
            &empirical_dataset.events_in_window,
            analysis_window,
            horizon_bucket,
        );
        let simulated_horizon_series = build_horizon_dataset_series(
            &simulated_dataset.events_in_window,
            analysis_window,
            horizon_bucket,
        );

        let open_jump_empirical = evaluate_tail_ratio_metric(
            &empirical_horizon_series.open_jump_abs_by_interval,
            empirical_horizon_series.open_jump_covered_interval_count,
            empirical_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .open_jump_abs_tail_expected_shortfall_ratio,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                1,
                100,
            ),
        );
        let open_jump_simulated = evaluate_tail_ratio_metric(
            &simulated_horizon_series.open_jump_abs_by_interval,
            simulated_horizon_series.open_jump_covered_interval_count,
            simulated_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .open_jump_abs_tail_expected_shortfall_ratio,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                2,
                100,
            ),
        );

        let closed_gap_empirical = evaluate_tail_ratio_metric(
            &empirical_horizon_series.closed_gap_abs_by_interval,
            empirical_horizon_series.closed_gap_covered_interval_count,
            empirical_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .closed_gap_abs_tail_expected_shortfall_ratio,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                1,
                200,
            ),
        );
        let closed_gap_simulated = evaluate_tail_ratio_metric(
            &simulated_horizon_series.closed_gap_abs_by_interval,
            simulated_horizon_series.closed_gap_covered_interval_count,
            simulated_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .closed_gap_abs_tail_expected_shortfall_ratio,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                2,
                200,
            ),
        );

        let contrast_empirical = evaluate_wasserstein_metric(
            &empirical_horizon_series.open_closed_contrast_by_interval,
            empirical_horizon_series.open_jump_covered_interval_count,
            empirical_horizon_series.closed_gap_covered_interval_count,
            empirical_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .closed_gap_vs_open_jump_wasserstein_distance,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                1,
                300,
            ),
        );
        let contrast_simulated = evaluate_wasserstein_metric(
            &simulated_horizon_series.open_closed_contrast_by_interval,
            simulated_horizon_series.open_jump_covered_interval_count,
            simulated_horizon_series.closed_gap_covered_interval_count,
            simulated_horizon_series.interval_count,
            request
                .sample_sufficiency_policy
                .closed_gap_vs_open_jump_wasserstein_distance,
            request.bootstrap_configuration,
            derive_metric_bootstrap_seed(
                request.bootstrap_configuration.random_seed,
                horizon_position,
                2,
                300,
            ),
        );

        horizons.push(GapBehaviorHorizonDiagnostics {
            horizon_label: horizon_bucket.horizon_label.clone(),
            horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            component_coupling_summary: build_component_coupling_summary_comparison(
                empirical_horizon_series.component_summary,
                simulated_horizon_series.component_summary,
            ),
            open_jump_abs_tail_expected_shortfall_ratio: build_metric_comparison(
                OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
                &metric_catalog_context.open_jump_metric_name,
                open_jump_empirical,
                open_jump_simulated,
                request
                    .threshold_policy
                    .open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta,
                &request.threshold_policy.comparison_policy_id,
            ),
            closed_gap_abs_tail_expected_shortfall_ratio: build_metric_comparison(
                CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
                &metric_catalog_context.closed_gap_metric_name,
                closed_gap_empirical,
                closed_gap_simulated,
                request
                    .threshold_policy
                    .closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta,
                &request.threshold_policy.comparison_policy_id,
            ),
            closed_gap_vs_open_jump_wasserstein_distance: build_metric_comparison(
                CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID,
                &metric_catalog_context.contrast_metric_name,
                contrast_empirical,
                contrast_simulated,
                request
                    .threshold_policy
                    .closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta,
                &request.threshold_policy.comparison_policy_id,
            ),
        });
    }

    Ok(GapBehaviorDiagnosticsReport {
        schema_version: GAP_BEHAVIOR_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        determinism_policy: GAP_BEHAVIOR_DIAGNOSTICS_DETERMINISM_POLICY,
        bootstrap_policy: GAP_BEHAVIOR_DIAGNOSTICS_BOOTSTRAP_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        metric_catalog_id: metric_catalog_context.catalog_id,
        metric_catalog_version: metric_catalog_context.catalog_version,
        empirical_dataset_id: empirical_dataset.dataset_id,
        simulated_dataset_id: simulated_dataset.dataset_id,
        analysis_start_utc_nanos: request.analysis_start_utc_nanos,
        analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        empirical_scenarios: empirical_dataset.scenario_metadata,
        simulated_scenarios: simulated_dataset.scenario_metadata,
        horizons,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PreparedGapBehaviorEvent {
    timestamp_utc_nanos: i64,
    market_state: GapBehaviorMarketState,
    closed_window_regime: Option<GapBehaviorClosedWindowRegime>,
    activity_coupling_increment: f64,
    open_jump_increment: f64,
    closed_gap_increment: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedGapBehaviorDataset {
    dataset_id: String,
    scenario_metadata: Vec<GapBehaviorScenarioMetadata>,
    events_in_window: Vec<PreparedGapBehaviorEvent>,
}

#[derive(Debug, Clone, PartialEq)]
struct GapBehaviorHorizonDatasetSeries {
    interval_count: usize,
    open_jump_abs_by_interval: Vec<f64>,
    closed_gap_abs_by_interval: Vec<f64>,
    open_closed_contrast_by_interval: Vec<OpenClosedContrastIntervalSample>,
    open_jump_covered_interval_count: usize,
    closed_gap_covered_interval_count: usize,
    component_summary: GapBehaviorComponentCouplingSummary,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OpenClosedContrastIntervalSample {
    open_jump_abs_value: f64,
    closed_gap_abs_value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TailRatioComputation {
    ratio: f64,
    tail_observation_count: usize,
}

fn validate_request(
    request: &GapBehaviorDiagnosticsRequest,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if request.analysis_start_utc_nanos >= request.analysis_end_utc_nanos {
        return Err(GapBehaviorDiagnosticsError::InvalidAnalysisWindow {
            analysis_start_utc_nanos: request.analysis_start_utc_nanos,
            analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        });
    }

    validate_dataset("empirical", &request.empirical_dataset)?;
    validate_dataset("simulated", &request.simulated_dataset)?;

    if request.horizon_buckets.is_empty() {
        return Err(GapBehaviorDiagnosticsError::EmptyHorizonBucketSet);
    }

    let mut observed_horizon_labels: BTreeSet<&str> = BTreeSet::new();
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        if horizon_bucket.horizon_label.trim().is_empty() {
            return Err(GapBehaviorDiagnosticsError::EmptyHorizonLabel { horizon_position });
        }
        if horizon_bucket.horizon_duration_nanos <= 0 {
            return Err(GapBehaviorDiagnosticsError::InvalidHorizonDuration {
                horizon_label: horizon_bucket.horizon_label.clone(),
                horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            });
        }
        if !observed_horizon_labels.insert(horizon_bucket.horizon_label.as_str()) {
            return Err(GapBehaviorDiagnosticsError::DuplicateHorizonLabel {
                horizon_label: horizon_bucket.horizon_label.clone(),
            });
        }
    }

    validate_bootstrap_configuration(request.bootstrap_configuration)?;
    request
        .sample_sufficiency_policy
        .open_jump_abs_tail_expected_shortfall_ratio
        .validate("open_jump_abs_tail_expected_shortfall_ratio")?;
    request
        .sample_sufficiency_policy
        .closed_gap_abs_tail_expected_shortfall_ratio
        .validate("closed_gap_abs_tail_expected_shortfall_ratio")?;
    request
        .sample_sufficiency_policy
        .closed_gap_vs_open_jump_wasserstein_distance
        .validate("closed_gap_vs_open_jump_wasserstein_distance")?;
    validate_threshold_policy(&request.threshold_policy)?;

    Ok(())
}

fn validate_dataset(
    dataset_role: &'static str,
    dataset: &GapBehaviorDatasetInput,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if dataset.dataset_id.trim().is_empty() {
        return Err(GapBehaviorDiagnosticsError::EmptyDatasetId { dataset_role });
    }

    if dataset.scenarios.is_empty() {
        return Err(GapBehaviorDiagnosticsError::EmptyScenarioSet { dataset_role });
    }

    let mut observed_scenario_ids: BTreeSet<&str> = BTreeSet::new();
    for (scenario_position, scenario) in dataset.scenarios.iter().enumerate() {
        if scenario.scenario_id.trim().is_empty() {
            return Err(GapBehaviorDiagnosticsError::EmptyScenarioId {
                dataset_role,
                scenario_position,
            });
        }

        if !observed_scenario_ids.insert(scenario.scenario_id.as_str()) {
            return Err(GapBehaviorDiagnosticsError::DuplicateScenarioId {
                dataset_role,
                scenario_id: scenario.scenario_id.clone(),
            });
        }

        if scenario.event_observations.is_empty() {
            return Err(GapBehaviorDiagnosticsError::EmptyScenarioEventSet {
                dataset_role,
                scenario_id: scenario.scenario_id.clone(),
            });
        }

        for (tag_position, tag) in scenario.tags.iter().enumerate() {
            if tag.trim().is_empty() {
                return Err(GapBehaviorDiagnosticsError::EmptyScenarioTag {
                    dataset_role,
                    scenario_id: scenario.scenario_id.clone(),
                    tag_position,
                });
            }
        }

        for (event_position, event_observation) in scenario.event_observations.iter().enumerate() {
            if event_observation.event_id.trim().is_empty() {
                return Err(GapBehaviorDiagnosticsError::EmptyEventId {
                    dataset_role,
                    scenario_id: scenario.scenario_id.clone(),
                    event_position,
                });
            }

            validate_finite_event_value(
                dataset_role,
                &scenario.scenario_id,
                &event_observation.event_id,
                "activity_coupling_increment",
                event_observation.activity_coupling_increment,
            )?;
            validate_finite_event_value(
                dataset_role,
                &scenario.scenario_id,
                &event_observation.event_id,
                "open_jump_increment",
                event_observation.open_jump_increment,
            )?;
            validate_finite_event_value(
                dataset_role,
                &scenario.scenario_id,
                &event_observation.event_id,
                "closed_gap_increment",
                event_observation.closed_gap_increment,
            )?;

            validate_routing_semantics(dataset_role, &scenario.scenario_id, event_observation)?;
        }
    }

    Ok(())
}

fn validate_finite_event_value(
    dataset_role: &'static str,
    scenario_id: &str,
    event_id: &str,
    field_name: &'static str,
    value: f64,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(GapBehaviorDiagnosticsError::InvalidEventObservation {
            dataset_role,
            scenario_id: scenario_id.to_string(),
            event_id: event_id.to_string(),
            field_name,
            value,
        })
    }
}

fn validate_routing_semantics(
    dataset_role: &'static str,
    scenario_id: &str,
    event_observation: &GapBehaviorEventObservation,
) -> Result<(), GapBehaviorDiagnosticsError> {
    match event_observation.market_state {
        GapBehaviorMarketState::Open => {
            if event_observation.closed_window_regime.is_some() {
                return Err(GapBehaviorDiagnosticsError::InvalidRoutingSemantics {
                    dataset_role,
                    scenario_id: scenario_id.to_string(),
                    event_id: event_observation.event_id.clone(),
                    details: "open-window events must not define closed_window_regime".to_string(),
                });
            }
            if event_observation.closed_gap_increment.abs() > ROUTING_TOLERANCE {
                return Err(GapBehaviorDiagnosticsError::InvalidRoutingSemantics {
                    dataset_role,
                    scenario_id: scenario_id.to_string(),
                    event_id: event_observation.event_id.clone(),
                    details: format!(
                        "open-window events must not emit closed-gap increments, found {}",
                        event_observation.closed_gap_increment
                    ),
                });
            }
        }
        GapBehaviorMarketState::Closed => {
            if event_observation.closed_window_regime.is_none() {
                return Err(GapBehaviorDiagnosticsError::InvalidRoutingSemantics {
                    dataset_role,
                    scenario_id: scenario_id.to_string(),
                    event_id: event_observation.event_id.clone(),
                    details: "closed-window events must define closed_window_regime".to_string(),
                });
            }
            if event_observation.open_jump_increment.abs() > ROUTING_TOLERANCE {
                return Err(GapBehaviorDiagnosticsError::InvalidRoutingSemantics {
                    dataset_role,
                    scenario_id: scenario_id.to_string(),
                    event_id: event_observation.event_id.clone(),
                    details: format!(
                        "closed-window events must not emit open-jump increments, found {}",
                        event_observation.open_jump_increment
                    ),
                });
            }
        }
    }

    Ok(())
}

fn validate_bootstrap_configuration(
    bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if let Err(field_name) = validate_block_bootstrap_configuration(BlockBootstrapConfiguration {
        confidence_level: bootstrap_configuration.confidence_level,
        replication_count: bootstrap_configuration.replication_count,
        block_length_interval_count: bootstrap_configuration.block_length_interval_count,
    }) {
        let details = match field_name {
            "confidence_level" => format!(
                "confidence_level must be finite and in (0, 1), found {}",
                bootstrap_configuration.confidence_level
            ),
            "replication_count" => "replication_count must be greater than 0".to_string(),
            "block_length_interval_count" => {
                "block_length_interval_count must be greater than 0".to_string()
            }
            _ => "invalid bootstrap configuration".to_string(),
        };

        return Err(GapBehaviorDiagnosticsError::InvalidBootstrapConfiguration {
            field_name,
            details,
        });
    }

    Ok(())
}

fn validate_threshold_policy(
    threshold_policy: &GapBehaviorThresholdPolicy,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if threshold_policy.comparison_policy_id.trim().is_empty() {
        return Err(GapBehaviorDiagnosticsError::InvalidThresholdPolicy {
            field_name: "comparison_policy_id",
            details: "comparison_policy_id must be non-empty".to_string(),
        });
    }

    validate_non_negative_finite_threshold(
        "open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta",
        threshold_policy.open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta,
    )?;
    validate_non_negative_finite_threshold(
        "closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta",
        threshold_policy.closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta,
    )?;
    validate_non_negative_finite_threshold(
        "closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta",
        threshold_policy.closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta,
    )?;

    Ok(())
}

fn validate_non_negative_finite_threshold(
    field_name: &'static str,
    threshold_value: f64,
) -> Result<(), GapBehaviorDiagnosticsError> {
    if !threshold_value.is_finite() || threshold_value < 0.0 {
        return Err(GapBehaviorDiagnosticsError::InvalidThresholdPolicy {
            field_name,
            details: format!(
                "threshold must be finite and non-negative, found {}",
                threshold_value
            ),
        });
    }

    Ok(())
}

fn resolve_metric_catalog_context()
-> Result<GapBehaviorMetricCatalogContext, GapBehaviorDiagnosticsError> {
    let catalog = build_stylized_fact_metric_catalog();

    let open_jump_metric = catalog
        .metrics
        .iter()
        .find(|metric| metric.metric_id == OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID)
        .ok_or(GapBehaviorDiagnosticsError::MetricCatalogMetricMissing {
            metric_id: OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
        })?;
    if open_jump_metric.domain != StylizedFactMetricDomain::OpenWindowDirectJumpBehavior {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
            details: "metric domain must be `open_window_direct_jump_behavior`".to_string(),
        });
    }
    if open_jump_metric.horizon_applicability
        != StylizedFactMetricHorizonApplicability::OpenWindowOnly
    {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
            details: "horizon_applicability must be `open_window_only`".to_string(),
        });
    }

    let closed_gap_metric = catalog
        .metrics
        .iter()
        .find(|metric| metric.metric_id == CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID)
        .ok_or(GapBehaviorDiagnosticsError::MetricCatalogMetricMissing {
            metric_id: CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
        })?;
    if closed_gap_metric.domain != StylizedFactMetricDomain::ClosedWindowGapBehavior {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
            details: "metric domain must be `closed_window_gap_behavior`".to_string(),
        });
    }
    if closed_gap_metric.horizon_applicability
        != StylizedFactMetricHorizonApplicability::ClosedWindowOnly
    {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID,
            details: "horizon_applicability must be `closed_window_only`".to_string(),
        });
    }

    let contrast_metric = catalog
        .metrics
        .iter()
        .find(|metric| metric.metric_id == CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID)
        .ok_or(GapBehaviorDiagnosticsError::MetricCatalogMetricMissing {
            metric_id: CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID,
        })?;
    if contrast_metric.metric_family != StylizedFactMetricFamily::DistributionDistance {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID,
            details: "metric family must be `distribution_distance`".to_string(),
        });
    }
    if contrast_metric.domain != StylizedFactMetricDomain::ClosedWindowGapBehavior {
        return Err(GapBehaviorDiagnosticsError::MetricCatalogMetricInvalid {
            metric_id: CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID,
            details: "metric domain must be `closed_window_gap_behavior`".to_string(),
        });
    }

    Ok(GapBehaviorMetricCatalogContext {
        catalog_id: catalog.catalog_id,
        catalog_version: catalog.catalog_version,
        open_jump_metric_name: open_jump_metric.metric_name.clone(),
        closed_gap_metric_name: closed_gap_metric.metric_name.clone(),
        contrast_metric_name: contrast_metric.metric_name.clone(),
    })
}

fn prepare_dataset(
    dataset_role: &'static str,
    dataset: &GapBehaviorDatasetInput,
    analysis_window: TimestampAnalysisWindow,
) -> Result<PreparedGapBehaviorDataset, GapBehaviorDiagnosticsError> {
    let mut scenario_metadata: Vec<GapBehaviorScenarioMetadata> =
        Vec::with_capacity(dataset.scenarios.len());
    let mut events_in_window: Vec<PreparedGapBehaviorEvent> = Vec::new();

    for scenario in &dataset.scenarios {
        let total_event_count = scenario.event_observations.len();
        let mut in_window_event_count = 0usize;
        let mut open_window_event_count = 0usize;
        let mut closed_window_event_count = 0usize;
        let mut overnight_closed_window_event_count = 0usize;
        let mut weekend_closed_window_event_count = 0usize;

        for event_observation in &scenario.event_observations {
            if !analysis_window.contains_timestamp(event_observation.timestamp_utc_nanos) {
                continue;
            }

            in_window_event_count = in_window_event_count.saturating_add(1);
            match event_observation.market_state {
                GapBehaviorMarketState::Open => {
                    open_window_event_count = open_window_event_count.saturating_add(1);
                }
                GapBehaviorMarketState::Closed => {
                    closed_window_event_count = closed_window_event_count.saturating_add(1);
                    match event_observation.closed_window_regime {
                        Some(GapBehaviorClosedWindowRegime::Overnight) => {
                            overnight_closed_window_event_count =
                                overnight_closed_window_event_count.saturating_add(1);
                        }
                        Some(GapBehaviorClosedWindowRegime::Weekend) => {
                            weekend_closed_window_event_count =
                                weekend_closed_window_event_count.saturating_add(1);
                        }
                        Some(GapBehaviorClosedWindowRegime::OtherClosed) | None => {}
                    }
                }
            }

            events_in_window.push(PreparedGapBehaviorEvent {
                timestamp_utc_nanos: event_observation.timestamp_utc_nanos,
                market_state: event_observation.market_state,
                closed_window_regime: event_observation.closed_window_regime,
                activity_coupling_increment: event_observation.activity_coupling_increment,
                open_jump_increment: event_observation.open_jump_increment,
                closed_gap_increment: event_observation.closed_gap_increment,
            });
        }

        let analysis_window_coverage_ratio = if total_event_count == 0 {
            0.0
        } else {
            in_window_event_count as f64 / total_event_count as f64
        };

        scenario_metadata.push(GapBehaviorScenarioMetadata {
            scenario_id: scenario.scenario_id.clone(),
            exchange_code: scenario.exchange_code.clone(),
            scenario_class: scenario.scenario_class,
            deterministic_replay_metadata: scenario.deterministic_replay_metadata.clone(),
            issue_traceability: scenario.issue_traceability.clone(),
            tags: scenario.tags.clone(),
            total_event_count,
            in_window_event_count,
            analysis_window_coverage_ratio,
            open_window_event_count,
            closed_window_event_count,
            overnight_closed_window_event_count,
            weekend_closed_window_event_count,
        });
    }

    if dataset_role == "empirical" || dataset_role == "simulated" {
        events_in_window.sort_by_key(|event| event.timestamp_utc_nanos);
    }

    Ok(PreparedGapBehaviorDataset {
        dataset_id: dataset.dataset_id.clone(),
        scenario_metadata,
        events_in_window,
    })
}

fn build_horizon_dataset_series(
    events_in_window: &[PreparedGapBehaviorEvent],
    analysis_window: TimestampAnalysisWindow,
    horizon_bucket: &GapBehaviorHorizonBucket,
) -> GapBehaviorHorizonDatasetSeries {
    let interval_count =
        interval_count_for_window(analysis_window, horizon_bucket.horizon_duration_nanos)
            .unwrap_or(0);

    let mut open_jump_sum_by_interval: Vec<f64> = vec![0.0; interval_count];
    let mut closed_gap_sum_by_interval: Vec<f64> = vec![0.0; interval_count];
    let mut open_event_count_by_interval: Vec<usize> = vec![0; interval_count];
    let mut closed_event_count_by_interval: Vec<usize> = vec![0; interval_count];

    let mut component_summary = GapBehaviorComponentCouplingSummary {
        open_window_event_count: 0,
        closed_window_event_count: 0,
        overnight_closed_window_event_count: 0,
        weekend_closed_window_event_count: 0,
        other_closed_window_event_count: 0,
        activity_coupling_total: 0.0,
        open_jump_total: 0.0,
        closed_gap_total: 0.0,
        overnight_closed_gap_total: 0.0,
        weekend_closed_gap_total: 0.0,
    };

    for event in events_in_window {
        let Some(interval_position) = interval_position_for_timestamp(
            event.timestamp_utc_nanos,
            analysis_window,
            horizon_bucket.horizon_duration_nanos,
            interval_count,
        ) else {
            continue;
        };

        component_summary.activity_coupling_total += event.activity_coupling_increment;
        component_summary.open_jump_total += event.open_jump_increment;
        component_summary.closed_gap_total += event.closed_gap_increment;

        match event.market_state {
            GapBehaviorMarketState::Open => {
                component_summary.open_window_event_count =
                    component_summary.open_window_event_count.saturating_add(1);
                open_jump_sum_by_interval[interval_position] += event.open_jump_increment;
                open_event_count_by_interval[interval_position] =
                    open_event_count_by_interval[interval_position].saturating_add(1);
            }
            GapBehaviorMarketState::Closed => {
                component_summary.closed_window_event_count = component_summary
                    .closed_window_event_count
                    .saturating_add(1);
                closed_gap_sum_by_interval[interval_position] += event.closed_gap_increment;
                closed_event_count_by_interval[interval_position] =
                    closed_event_count_by_interval[interval_position].saturating_add(1);

                match event.closed_window_regime {
                    Some(GapBehaviorClosedWindowRegime::Overnight) => {
                        component_summary.overnight_closed_window_event_count = component_summary
                            .overnight_closed_window_event_count
                            .saturating_add(1);
                        component_summary.overnight_closed_gap_total += event.closed_gap_increment;
                    }
                    Some(GapBehaviorClosedWindowRegime::Weekend) => {
                        component_summary.weekend_closed_window_event_count = component_summary
                            .weekend_closed_window_event_count
                            .saturating_add(1);
                        component_summary.weekend_closed_gap_total += event.closed_gap_increment;
                    }
                    Some(GapBehaviorClosedWindowRegime::OtherClosed) | None => {
                        component_summary.other_closed_window_event_count = component_summary
                            .other_closed_window_event_count
                            .saturating_add(1);
                    }
                }
            }
        }
    }

    let mut open_jump_abs_by_interval: Vec<f64> = Vec::new();
    let mut closed_gap_abs_by_interval: Vec<f64> = Vec::new();
    let mut open_closed_contrast_by_interval: Vec<OpenClosedContrastIntervalSample> = Vec::new();

    for interval_position in 0..interval_count {
        let open_interval_active = open_event_count_by_interval[interval_position] > 0;
        let closed_interval_active = closed_event_count_by_interval[interval_position] > 0;

        if open_interval_active {
            open_jump_abs_by_interval.push(open_jump_sum_by_interval[interval_position].abs());
        }

        if closed_interval_active {
            closed_gap_abs_by_interval.push(closed_gap_sum_by_interval[interval_position].abs());
        }

        if open_interval_active || closed_interval_active {
            let open_jump_abs_value = if open_interval_active {
                open_jump_sum_by_interval[interval_position].abs()
            } else {
                0.0
            };
            let closed_gap_abs_value = if closed_interval_active {
                closed_gap_sum_by_interval[interval_position].abs()
            } else {
                0.0
            };

            open_closed_contrast_by_interval.push(OpenClosedContrastIntervalSample {
                open_jump_abs_value,
                closed_gap_abs_value,
            });
        }
    }

    GapBehaviorHorizonDatasetSeries {
        interval_count,
        open_jump_covered_interval_count: open_jump_abs_by_interval.len(),
        closed_gap_covered_interval_count: closed_gap_abs_by_interval.len(),
        open_jump_abs_by_interval,
        closed_gap_abs_by_interval,
        open_closed_contrast_by_interval,
        component_summary,
    }
}

fn evaluate_tail_ratio_metric(
    interval_abs_series: &[f64],
    covered_interval_count: usize,
    horizon_interval_count: usize,
    required_minimums: GapBehaviorMetricSampleSufficiencyPolicy,
    bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> GapBehaviorDatasetMetricResult {
    let computation = compute_abs_tail_expected_shortfall_ratio(interval_abs_series);
    let value = computation.map(|tail_ratio| tail_ratio.ratio);

    let observed_counts = GapBehaviorObservedCounts {
        sample_count: interval_abs_series.len(),
        effective_sample_count: interval_abs_series.len(),
        tail_observation_count: computation
            .map(|tail_ratio| tail_ratio.tail_observation_count)
            .unwrap_or(0),
    };

    let sample_coverage = GapBehaviorSampleCoverage {
        horizon_interval_count,
        covered_interval_count,
    };

    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );

    let status = resolve_metric_status(
        value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = compute_confidence_interval_if_available(
        status,
        value,
        interval_abs_series,
        bootstrap_configuration,
        bootstrap_seed,
        compute_abs_tail_expected_shortfall_ratio_from_series,
    );

    GapBehaviorDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn evaluate_wasserstein_metric(
    contrast_interval_series: &[OpenClosedContrastIntervalSample],
    open_covered_interval_count: usize,
    closed_covered_interval_count: usize,
    horizon_interval_count: usize,
    required_minimums: GapBehaviorMetricSampleSufficiencyPolicy,
    bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> GapBehaviorDatasetMetricResult {
    let value = compute_open_closed_wasserstein_distance(contrast_interval_series);
    let effective_sample_count = open_covered_interval_count.min(closed_covered_interval_count);

    let observed_counts = GapBehaviorObservedCounts {
        sample_count: contrast_interval_series.len(),
        effective_sample_count,
        tail_observation_count: 0,
    };

    let sample_coverage = GapBehaviorSampleCoverage {
        horizon_interval_count,
        covered_interval_count: effective_sample_count,
    };

    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );

    let status = resolve_metric_status(
        value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = compute_confidence_interval_if_available(
        status,
        value,
        contrast_interval_series,
        bootstrap_configuration,
        bootstrap_seed,
        compute_open_closed_wasserstein_distance,
    );

    GapBehaviorDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn compute_abs_tail_expected_shortfall_ratio_from_series(
    interval_abs_series: &[f64],
) -> Option<f64> {
    compute_abs_tail_expected_shortfall_ratio(interval_abs_series)
        .map(|tail_ratio| tail_ratio.ratio)
}

fn block_bootstrap_configuration(
    bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
) -> BlockBootstrapConfiguration {
    BlockBootstrapConfiguration {
        confidence_level: bootstrap_configuration.confidence_level,
        replication_count: bootstrap_configuration.replication_count,
        block_length_interval_count: bootstrap_configuration.block_length_interval_count,
    }
}

fn compute_confidence_interval_if_available<T, F>(
    status: GapBehaviorMetricStatus,
    value: Option<f64>,
    source_series: &[T],
    bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration,
    bootstrap_seed: u64,
    metric_evaluator: F,
) -> Option<GapBehaviorConfidenceInterval>
where
    T: Copy,
    F: Fn(&[T]) -> Option<f64>,
{
    if status != GapBehaviorMetricStatus::Available {
        return None;
    }

    value.map(|point_estimate| {
        let (lower_bound, upper_bound) = compute_block_bootstrap_confidence_interval(
            source_series,
            point_estimate,
            block_bootstrap_configuration(bootstrap_configuration),
            bootstrap_seed,
            metric_evaluator,
        );
        GapBehaviorConfidenceInterval {
            lower_bound,
            upper_bound,
            confidence_level: bootstrap_configuration.confidence_level,
        }
    })
}

fn compute_abs_tail_expected_shortfall_ratio(
    interval_abs_series: &[f64],
) -> Option<TailRatioComputation> {
    if interval_abs_series.is_empty() {
        return None;
    }
    if interval_abs_series
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }

    let mut sorted_values: Vec<f64> = interval_abs_series.to_vec();
    sorted_values.sort_by(f64::total_cmp);

    let value_at_risk = compute_sorted_quantile(&sorted_values, TAIL_QUANTILE_LEVEL);
    if !value_at_risk.is_finite() || value_at_risk <= MINIMUM_DENOMINATOR_MAGNITUDE {
        return None;
    }

    let tail_values: Vec<f64> = sorted_values
        .iter()
        .copied()
        .filter(|value| *value >= value_at_risk)
        .collect();
    if tail_values.is_empty() {
        return None;
    }

    let expected_shortfall = tail_values.iter().sum::<f64>() / tail_values.len() as f64;
    if !expected_shortfall.is_finite() {
        return None;
    }

    let ratio = expected_shortfall / value_at_risk;
    if !ratio.is_finite() {
        return None;
    }

    Some(TailRatioComputation {
        ratio,
        tail_observation_count: tail_values.len(),
    })
}

fn compute_open_closed_wasserstein_distance(
    contrast_interval_series: &[OpenClosedContrastIntervalSample],
) -> Option<f64> {
    if contrast_interval_series.is_empty() {
        return None;
    }

    let mut open_jump_values: Vec<f64> = Vec::with_capacity(contrast_interval_series.len());
    let mut closed_gap_values: Vec<f64> = Vec::with_capacity(contrast_interval_series.len());
    for sample in contrast_interval_series {
        if !sample.open_jump_abs_value.is_finite()
            || !sample.closed_gap_abs_value.is_finite()
            || sample.open_jump_abs_value < 0.0
            || sample.closed_gap_abs_value < 0.0
        {
            return None;
        }

        open_jump_values.push(sample.open_jump_abs_value);
        closed_gap_values.push(sample.closed_gap_abs_value);
    }

    open_jump_values.sort_by(f64::total_cmp);
    closed_gap_values.sort_by(f64::total_cmp);

    let pair_count = open_jump_values.len();
    if pair_count == 0 || pair_count != closed_gap_values.len() {
        return None;
    }

    let total_distance = open_jump_values
        .iter()
        .zip(closed_gap_values.iter())
        .map(|(open_jump_value, closed_gap_value)| (open_jump_value - closed_gap_value).abs())
        .sum::<f64>();

    let distance = total_distance / pair_count as f64;
    distance.is_finite().then_some(distance)
}

fn resolve_metric_status(
    value: Option<f64>,
    observed_counts: GapBehaviorObservedCounts,
    horizon_coverage_ratio: f64,
    required_minimums: GapBehaviorMetricSampleSufficiencyPolicy,
) -> GapBehaviorMetricStatus {
    let has_sufficient_counts = observed_counts.sample_count
        >= required_minimums.minimum_sample_count
        && observed_counts.effective_sample_count
            >= required_minimums.minimum_effective_sample_count
        && observed_counts.tail_observation_count
            >= required_minimums.minimum_tail_observation_count;
    let has_sufficient_coverage =
        horizon_coverage_ratio >= required_minimums.minimum_horizon_coverage_ratio;

    if value.is_some() && has_sufficient_counts && has_sufficient_coverage {
        GapBehaviorMetricStatus::Available
    } else {
        GapBehaviorMetricStatus::InsufficientData
    }
}

fn build_metric_comparison(
    metric_id: &str,
    metric_name: &str,
    empirical: GapBehaviorDatasetMetricResult,
    simulated: GapBehaviorDatasetMetricResult,
    max_absolute_delta: f64,
    comparison_policy_id: &str,
) -> GapBehaviorMetricComparison {
    let absolute_delta = match (
        empirical.status,
        empirical.value,
        simulated.status,
        simulated.value,
    ) {
        (
            GapBehaviorMetricStatus::Available,
            Some(empirical_value),
            GapBehaviorMetricStatus::Available,
            Some(simulated_value),
        ) => Some((empirical_value - simulated_value).abs()),
        _ => None,
    };

    let status = if absolute_delta.is_some() {
        GapBehaviorMetricStatus::Available
    } else {
        GapBehaviorMetricStatus::InsufficientData
    };

    let uncertainty_context = build_delta_uncertainty_context(
        empirical.confidence_interval.map(|confidence_interval| {
            (
                confidence_interval.lower_bound,
                confidence_interval.upper_bound,
            )
        }),
        simulated.confidence_interval.map(|confidence_interval| {
            (
                confidence_interval.lower_bound,
                confidence_interval.upper_bound,
            )
        }),
        absolute_delta,
        |uncertainty_components| GapBehaviorDeltaUncertaintyContext {
            empirical_interval_half_width: uncertainty_components.empirical_interval_half_width,
            simulated_interval_half_width: uncertainty_components.simulated_interval_half_width,
            pooled_interval_half_width: uncertainty_components.pooled_interval_half_width,
            standardized_delta: uncertainty_components.standardized_delta,
        },
    );

    let absolute_delta_exceeds_threshold = absolute_delta.map(|delta| delta > max_absolute_delta);

    GapBehaviorMetricComparison {
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        status,
        empirical,
        simulated,
        absolute_delta,
        uncertainty_context,
        threshold_context: GapBehaviorThresholdContext {
            comparison_policy_id: comparison_policy_id.to_string(),
            max_absolute_delta,
            absolute_delta_exceeds_threshold,
        },
    }
}

fn build_component_coupling_summary_comparison(
    empirical: GapBehaviorComponentCouplingSummary,
    simulated: GapBehaviorComponentCouplingSummary,
) -> GapBehaviorComponentCouplingSummaryComparison {
    GapBehaviorComponentCouplingSummaryComparison {
        empirical,
        simulated,
        activity_coupling_absolute_delta: (empirical.activity_coupling_total
            - simulated.activity_coupling_total)
            .abs(),
        open_jump_absolute_delta: (empirical.open_jump_total - simulated.open_jump_total).abs(),
        closed_gap_absolute_delta: (empirical.closed_gap_total - simulated.closed_gap_total).abs(),
    }
}
