//! Fine-path aggregation helpers for coarse-versus-fine scale-consistency workflows.

use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_engine_sufficient_statistics::{
    SufficientStatisticsIntervalStatistics, SufficientStatisticsPath,
    SufficientStatisticsPathError, SufficientStatisticsPathRecord,
    SufficientStatisticsPathRecordError, SufficientStatisticsReturnComponents,
};
use serde::{Deserialize, Serialize};

use super::interval_grid::{PartitionedIntervalPair, PartitionedIntervalPairError};

/// Schema identifier for fine-path alignment metadata emitted by the aggregation engine.
pub const SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION: &str =
    "validation.scale_consistency.fine_path_alignment_metadata.v1";

/// Boundary policy identifier for deterministic fine-to-coarse interval alignment.
pub const SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_BOUNDARY_POLICY: &str =
    "sequential_interval_index_partitioning_with_half_open_coarse_windows.v1";

/// Policy controlling how trailing fine intervals that do not fill a whole coarse window behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyFinePathPartialWindowPolicy {
    Reject,
    DropTrailing,
}

/// Alignment outcome captured for auditability on aggregated fine-path outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyFinePathAlignmentStatus {
    ExactBoundaryMatch,
    TrailingPartialWindowDropped,
}

/// Machine-readable metadata describing how a fine path was aligned onto a coarse grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyFinePathAlignmentMetadata {
    pub schema_version: String,
    pub boundary_alignment_policy: String,
    pub partial_window_policy: ScaleConsistencyFinePathPartialWindowPolicy,
    pub alignment_status: ScaleConsistencyFinePathAlignmentStatus,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub source_fine_interval_count: usize,
    pub aggregated_coarse_interval_count: usize,
    pub trailing_partial_fine_interval_count: usize,
}

/// Aggregated coarse-grid view derived deterministically from one fine sufficient-statistics path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyAlignedRiskPath {
    pub alignment_metadata: ScaleConsistencyFinePathAlignmentMetadata,
    pub aggregated_path: SufficientStatisticsPath,
}

/// Aggregate a fine-grid sufficient-statistics path onto a coarser interval grid for later metric comparison.
pub fn aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
    fine_path: &SufficientStatisticsPath,
    coarse_interval_delta: f64,
    partial_window_policy: ScaleConsistencyFinePathPartialWindowPolicy,
) -> Result<ScaleConsistencyAlignedRiskPath, ScaleConsistencyFinePathAggregationError> {
    fine_path
        .validate()
        .map_err(|source| ScaleConsistencyFinePathAggregationError::InvalidFinePath { source })?;

    let interval_pair =
        PartitionedIntervalPair::try_new(coarse_interval_delta, fine_path.interval_delta)
            .map_err(map_interval_pair_error)?;

    let source_fine_interval_count = fine_path.records.len();
    let fine_subinterval_count_per_coarse_interval =
        interval_pair.fine_subinterval_count_per_coarse_interval;
    let aggregated_coarse_interval_count =
        source_fine_interval_count / fine_subinterval_count_per_coarse_interval;
    let trailing_partial_fine_interval_count =
        source_fine_interval_count % fine_subinterval_count_per_coarse_interval;

    let alignment_status = match (trailing_partial_fine_interval_count, partial_window_policy) {
        (0, _) => ScaleConsistencyFinePathAlignmentStatus::ExactBoundaryMatch,
        (
            trailing_partial_fine_interval_count,
            ScaleConsistencyFinePathPartialWindowPolicy::Reject,
        ) => {
            return Err(
                ScaleConsistencyFinePathAggregationError::TrailingPartialWindowRejected {
                    source_fine_interval_count,
                    fine_subinterval_count_per_coarse_interval,
                    trailing_partial_fine_interval_count,
                },
            );
        }
        (_, ScaleConsistencyFinePathPartialWindowPolicy::DropTrailing) => {
            ScaleConsistencyFinePathAlignmentStatus::TrailingPartialWindowDropped
        }
    };

    if aggregated_coarse_interval_count == 0 {
        return Err(
            ScaleConsistencyFinePathAggregationError::NoCompleteCoarseIntervalsAfterAlignment {
                source_fine_interval_count,
                fine_subinterval_count_per_coarse_interval,
                partial_window_policy,
            },
        );
    }

    let mut aggregated_records: Vec<SufficientStatisticsPathRecord> =
        Vec::with_capacity(aggregated_coarse_interval_count);

    for coarse_interval_index in 0..aggregated_coarse_interval_count {
        let fine_start =
            coarse_interval_index.saturating_mul(fine_subinterval_count_per_coarse_interval);
        let fine_end = fine_start + fine_subinterval_count_per_coarse_interval;

        let aggregated_record = aggregate_fine_records_to_coarse_interval(
            coarse_interval_index,
            &fine_path.records[fine_start..fine_end],
        )
        .map_err(|source| {
            ScaleConsistencyFinePathAggregationError::InvalidAggregatedRecord {
                coarse_interval_index,
                source,
            }
        })?;

        aggregated_records.push(aggregated_record);
    }

    let aggregated_path = SufficientStatisticsPath::try_new(
        fine_path.seed,
        coarse_interval_delta,
        aggregated_coarse_interval_count,
        fine_path.initial_excitation_state,
        fine_path.terminal_excitation_state,
        aggregated_records,
    )
    .map_err(|source| ScaleConsistencyFinePathAggregationError::InvalidAggregatedPath { source })?;

    Ok(ScaleConsistencyAlignedRiskPath {
        alignment_metadata: ScaleConsistencyFinePathAlignmentMetadata {
            schema_version: SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION
                .to_string(),
            boundary_alignment_policy: SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_BOUNDARY_POLICY
                .to_string(),
            partial_window_policy,
            alignment_status,
            coarse_interval_delta,
            fine_interval_delta: fine_path.interval_delta,
            fine_subinterval_count_per_coarse_interval,
            source_fine_interval_count,
            aggregated_coarse_interval_count,
            trailing_partial_fine_interval_count,
        },
        aggregated_path,
    })
}

/// Errors returned when fine-path aggregation cannot produce a valid coarse-aligned output.
#[derive(Debug, PartialEq)]
pub enum ScaleConsistencyFinePathAggregationError {
    InvalidFinePath {
        source: SufficientStatisticsPathError,
    },
    InvalidCoarseIntervalDelta {
        value: f64,
    },
    InvalidFineIntervalDelta {
        value: f64,
    },
    FineIntervalMustBeStrictlySmaller {
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
    },
    IntervalDeltaRatioMismatch {
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
        implied_ratio: f64,
    },
    TrailingPartialWindowRejected {
        source_fine_interval_count: usize,
        fine_subinterval_count_per_coarse_interval: usize,
        trailing_partial_fine_interval_count: usize,
    },
    NoCompleteCoarseIntervalsAfterAlignment {
        source_fine_interval_count: usize,
        fine_subinterval_count_per_coarse_interval: usize,
        partial_window_policy: ScaleConsistencyFinePathPartialWindowPolicy,
    },
    InvalidAggregatedRecord {
        coarse_interval_index: usize,
        source: SufficientStatisticsPathRecordError,
    },
    InvalidAggregatedPath {
        source: SufficientStatisticsPathError,
    },
}

impl Display for ScaleConsistencyFinePathAggregationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFinePath { source } => {
                write!(formatter, "fine_path is invalid: {source}")
            }
            Self::InvalidCoarseIntervalDelta { value } => write!(
                formatter,
                "coarse_interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidFineIntervalDelta { value } => write!(
                formatter,
                "fine_interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::FineIntervalMustBeStrictlySmaller {
                coarse_interval_delta,
                fine_interval_delta,
            } => write!(
                formatter,
                "fine_interval_delta must be strictly smaller than coarse_interval_delta, found coarse_interval_delta={coarse_interval_delta} and fine_interval_delta={fine_interval_delta}"
            ),
            Self::IntervalDeltaRatioMismatch {
                coarse_interval_delta,
                fine_interval_delta,
                implied_ratio,
            } => write!(
                formatter,
                "coarse_interval_delta must be an exact integer multiple of fine_interval_delta, found coarse_interval_delta={coarse_interval_delta}, fine_interval_delta={fine_interval_delta}, implied_ratio={implied_ratio}"
            ),
            Self::TrailingPartialWindowRejected {
                source_fine_interval_count,
                fine_subinterval_count_per_coarse_interval,
                trailing_partial_fine_interval_count,
            } => write!(
                formatter,
                "fine_path contains a trailing partial coarse window and partial_window_policy=`reject`, found source_fine_interval_count={source_fine_interval_count}, fine_subinterval_count_per_coarse_interval={fine_subinterval_count_per_coarse_interval}, trailing_partial_fine_interval_count={trailing_partial_fine_interval_count}"
            ),
            Self::NoCompleteCoarseIntervalsAfterAlignment {
                source_fine_interval_count,
                fine_subinterval_count_per_coarse_interval,
                partial_window_policy,
            } => write!(
                formatter,
                "fine_path did not produce any complete coarse intervals after alignment, found source_fine_interval_count={source_fine_interval_count}, fine_subinterval_count_per_coarse_interval={fine_subinterval_count_per_coarse_interval}, partial_window_policy=`{partial_window_policy:?}`"
            ),
            Self::InvalidAggregatedRecord {
                coarse_interval_index,
                source,
            } => write!(
                formatter,
                "aggregated coarse record at interval_index={coarse_interval_index} is invalid: {source}"
            ),
            Self::InvalidAggregatedPath { source } => {
                write!(formatter, "aggregated coarse path is invalid: {source}")
            }
        }
    }
}

impl Error for ScaleConsistencyFinePathAggregationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidFinePath { source } => Some(source),
            Self::InvalidAggregatedRecord { source, .. } => Some(source),
            Self::InvalidAggregatedPath { source } => Some(source),
            _ => None,
        }
    }
}

fn map_interval_pair_error(
    source: PartitionedIntervalPairError,
) -> ScaleConsistencyFinePathAggregationError {
    match source {
        PartitionedIntervalPairError::InvalidCoarseIntervalDelta { value } => {
            ScaleConsistencyFinePathAggregationError::InvalidCoarseIntervalDelta { value }
        }
        PartitionedIntervalPairError::InvalidFineIntervalDelta { value } => {
            ScaleConsistencyFinePathAggregationError::InvalidFineIntervalDelta { value }
        }
        PartitionedIntervalPairError::FineIntervalMustBeStrictlySmaller {
            coarse_interval_delta,
            fine_interval_delta,
        } => ScaleConsistencyFinePathAggregationError::FineIntervalMustBeStrictlySmaller {
            coarse_interval_delta,
            fine_interval_delta,
        },
        PartitionedIntervalPairError::IntervalDeltaRatioMismatch {
            coarse_interval_delta,
            fine_interval_delta,
            implied_ratio,
        } => ScaleConsistencyFinePathAggregationError::IntervalDeltaRatioMismatch {
            coarse_interval_delta,
            fine_interval_delta,
            implied_ratio,
        },
    }
}

fn aggregate_fine_records_to_coarse_interval(
    coarse_interval_index: usize,
    fine_records: &[SufficientStatisticsPathRecord],
) -> Result<SufficientStatisticsPathRecord, SufficientStatisticsPathRecordError> {
    let aggregated_statistics = SufficientStatisticsIntervalStatistics::try_new(
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.baseline_a_plus)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.endogenous_a_plus)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.baseline_a_minus)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.endogenous_a_minus)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.n_plus)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.sufficient_statistics.n_minus)
            .sum(),
    )
    .map_err(
        |source| SufficientStatisticsPathRecordError::InvalidSufficientStatistics { source },
    )?;

    let aggregated_return_components = SufficientStatisticsReturnComponents {
        buy_side_return: fine_records
            .iter()
            .map(|record| record.return_components.buy_side_return)
            .sum(),
        sell_side_return: fine_records
            .iter()
            .map(|record| record.return_components.sell_side_return)
            .sum(),
        trading_time_return: fine_records
            .iter()
            .map(|record| record.return_components.trading_time_return)
            .sum(),
        open_jump_return: fine_records
            .iter()
            .map(|record| record.return_components.open_jump_return)
            .sum(),
        closed_gap_return: fine_records
            .iter()
            .map(|record| record.return_components.closed_gap_return)
            .sum(),
        total_return: fine_records
            .iter()
            .map(|record| record.return_components.total_return)
            .sum(),
    };
    let last_record = &fine_records[fine_records.len() - 1];

    SufficientStatisticsPathRecord::try_new(
        coarse_interval_index,
        aggregated_statistics,
        aggregated_return_components,
        fine_records
            .iter()
            .map(|record| record.core_total_return)
            .sum(),
        fine_records
            .iter()
            .map(|record| record.observed_interval_return)
            .sum(),
        last_record.observed_log_price_end,
        last_record.anchor_value_end,
        last_record.pressure_state_end,
        last_record.deviation_from_anchor_end,
    )
}
