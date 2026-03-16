//! Public contracts for scale-consistency metric delta calculation.

use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_engine_risk::SufficientStatisticsRiskPathError;
use serde::{Deserialize, Serialize};

use super::super::aggregation::{
    SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION, ScaleConsistencyAlignedRiskPath,
};

pub const SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION: &str =
    "validation.scale_consistency.metric_delta_batch_report.v1";
pub const SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY: &str =
    "preserve_request_scenario_order_and_metric_specification_order.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyMetricSpecification {
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lag_interval_counts: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyMetricScenarioInput {
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub horizon_label: String,
    pub coarse_path: janiform_engine_risk::SufficientStatisticsRiskPath,
    pub aligned_fine_path: ScaleConsistencyAlignedRiskPath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyMetricDeltaBatchRequest {
    pub metric_specifications: Vec<ScaleConsistencyMetricSpecification>,
    pub scenarios: Vec<ScaleConsistencyMetricScenarioInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyMetricDeltaBatchReport {
    pub schema_version: &'static str,
    pub ordering_policy: &'static str,
    pub scenario_reports: Vec<ScaleConsistencyMetricDeltaScenarioReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyMetricDeltaScenarioReport {
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub horizon_label: String,
    pub horizon_interval_count: usize,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub metric_deltas: Vec<ScaleConsistencyMetricDelta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyMetricCategory {
    Returns,
    Volatility,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyMetricDeltaKind {
    SignedDifference,
    AbsoluteDistance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyMetricComputationStatus {
    Computed,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyMetricDatasetRole {
    Coarse,
    Fine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyMetricFailureReasonCode {
    InsufficientReturnObservations,
    InsufficientLaggedObservations,
    MissingOpenJumpObservations,
    MissingClosedGapObservations,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyMetricComputationFailure {
    pub dataset_role: ScaleConsistencyMetricDatasetRole,
    pub reason_code: ScaleConsistencyMetricFailureReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lag_interval_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyMetricDelta {
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub metric_category: ScaleConsistencyMetricCategory,
    pub delta_kind: ScaleConsistencyMetricDeltaKind,
    pub horizon_label: String,
    pub horizon_interval_count: usize,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub status: ScaleConsistencyMetricComputationStatus,
    pub coarse_value: Option<f64>,
    pub fine_value: Option<f64>,
    pub delta_value: Option<f64>,
    pub absolute_delta: Option<f64>,
    pub computation_failures: Vec<ScaleConsistencyMetricComputationFailure>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyMetricDeltaBatchError {
    MissingMetricSpecifications,
    InvalidMetricSpecification {
        position: usize,
        source: ScaleConsistencyMetricSpecificationError,
    },
    DuplicateMetricSpecification {
        metric_id: String,
        metric_variant: Option<String>,
    },
    MissingScenarios,
    InvalidScenario {
        scenario_id: String,
        source: ScaleConsistencyMetricScenarioInputError,
    },
    DuplicateScenarioId {
        scenario_id: String,
    },
}

impl Display for ScaleConsistencyMetricDeltaBatchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMetricSpecifications => write!(
                formatter,
                "metric_specifications must contain at least one configured metric"
            ),
            Self::InvalidMetricSpecification { position, source } => write!(
                formatter,
                "metric_specifications[{position}] is invalid: {source}"
            ),
            Self::DuplicateMetricSpecification {
                metric_id,
                metric_variant,
            } => write!(
                formatter,
                "metric_specifications must be unique by metric_id and metric_variant, found duplicate metric_id=`{metric_id}` and metric_variant={metric_variant:?}"
            ),
            Self::MissingScenarios => {
                write!(
                    formatter,
                    "scenarios must contain at least one configured scenario"
                )
            }
            Self::InvalidScenario {
                scenario_id,
                source,
            } => write!(formatter, "scenario `{scenario_id}` is invalid: {source}"),
            Self::DuplicateScenarioId { scenario_id } => {
                write!(formatter, "duplicate scenario_id found: {scenario_id}")
            }
        }
    }
}

impl Error for ScaleConsistencyMetricDeltaBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMetricSpecification { source, .. } => Some(source),
            Self::InvalidScenario { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyMetricSpecificationError {
    MissingMetricId,
    EmptyMetricVariant,
    MetricNotPresentInStylizedFactCatalog { metric_id: String },
    MetricUnsupportedByScaleConsistency { metric_id: String },
    MissingLagIntervalCounts { metric_id: String },
    UnexpectedLagIntervalCounts { metric_id: String },
    InvalidLagIntervalCount { metric_id: String, value: usize },
    DuplicateLagIntervalCount { metric_id: String, value: usize },
}

impl Display for ScaleConsistencyMetricSpecificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMetricId => write!(formatter, "metric_id must be a non-empty string"),
            Self::EmptyMetricVariant => {
                write!(
                    formatter,
                    "metric_variant must be a non-empty string when provided"
                )
            }
            Self::MetricNotPresentInStylizedFactCatalog { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` is not present in the stylized-fact metric catalog"
            ),
            Self::MetricUnsupportedByScaleConsistency { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` is not supported by the scale-consistency metric calculator"
            ),
            Self::MissingLagIntervalCounts { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` requires lag_interval_counts"
            ),
            Self::UnexpectedLagIntervalCounts { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` does not accept lag_interval_counts"
            ),
            Self::InvalidLagIntervalCount { metric_id, value } => write!(
                formatter,
                "metric_id `{metric_id}` requires strictly positive lag_interval_counts, found {value}"
            ),
            Self::DuplicateLagIntervalCount { metric_id, value } => write!(
                formatter,
                "metric_id `{metric_id}` contains duplicate lag_interval_count {value}"
            ),
        }
    }
}

impl Error for ScaleConsistencyMetricSpecificationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyMetricScenarioInputError {
    MissingScenarioId,
    MissingParameterSetId,
    MissingHorizonLabel,
    InvalidCoarsePath {
        source: SufficientStatisticsRiskPathError,
    },
    InvalidAlignedFinePath {
        source: SufficientStatisticsRiskPathError,
    },
    InvalidAlignmentMetadataSchemaVersion {
        actual: String,
    },
    CoarseAndFineAggregatedIntervalDeltaMismatch {
        coarse_interval_delta: f64,
        fine_aggregated_interval_delta: f64,
    },
    CoarseAndFineIntervalCountMismatch {
        coarse_interval_count: usize,
        fine_interval_count: usize,
    },
    AlignmentMetadataCoarseDeltaMismatch {
        coarse_interval_delta: f64,
        metadata_coarse_interval_delta: f64,
    },
    AlignmentMetadataIntervalCountMismatch {
        coarse_interval_count: usize,
        metadata_aggregated_interval_count: usize,
    },
}

impl Display for ScaleConsistencyMetricScenarioInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingScenarioId => write!(formatter, "scenario_id must be a non-empty string"),
            Self::MissingParameterSetId => {
                write!(formatter, "parameter_set_id must be a non-empty string")
            }
            Self::MissingHorizonLabel => {
                write!(formatter, "horizon_label must be a non-empty string")
            }
            Self::InvalidCoarsePath { source } => {
                write!(formatter, "coarse_path is invalid: {source}")
            }
            Self::InvalidAlignedFinePath { source } => {
                write!(
                    formatter,
                    "aligned_fine_path.aggregated_path is invalid: {source}"
                )
            }
            Self::InvalidAlignmentMetadataSchemaVersion { actual } => write!(
                formatter,
                "aligned_fine_path.alignment_metadata.schema_version must be `{SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION}`, found `{actual}`"
            ),
            Self::CoarseAndFineAggregatedIntervalDeltaMismatch {
                coarse_interval_delta,
                fine_aggregated_interval_delta,
            } => write!(
                formatter,
                "coarse_path.interval_delta ({coarse_interval_delta}) must match aligned_fine_path.aggregated_path.interval_delta ({fine_aggregated_interval_delta})"
            ),
            Self::CoarseAndFineIntervalCountMismatch {
                coarse_interval_count,
                fine_interval_count,
            } => write!(
                formatter,
                "coarse_path.simulated_interval_count ({coarse_interval_count}) must match aligned_fine_path.aggregated_path.simulated_interval_count ({fine_interval_count})"
            ),
            Self::AlignmentMetadataCoarseDeltaMismatch {
                coarse_interval_delta,
                metadata_coarse_interval_delta,
            } => write!(
                formatter,
                "aligned_fine_path.alignment_metadata.coarse_interval_delta ({metadata_coarse_interval_delta}) must match coarse_path.interval_delta ({coarse_interval_delta})"
            ),
            Self::AlignmentMetadataIntervalCountMismatch {
                coarse_interval_count,
                metadata_aggregated_interval_count,
            } => write!(
                formatter,
                "aligned_fine_path.alignment_metadata.aggregated_coarse_interval_count ({metadata_aggregated_interval_count}) must match coarse_path.simulated_interval_count ({coarse_interval_count})"
            ),
        }
    }
}

impl Error for ScaleConsistencyMetricScenarioInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCoarsePath { source } => Some(source),
            Self::InvalidAlignedFinePath { source } => Some(source),
            _ => None,
        }
    }
}
