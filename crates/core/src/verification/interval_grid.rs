use std::error::Error;
use std::fmt::{Display, Formatter};

const PARTITIONED_INTERVAL_RATIO_TOLERANCE: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PartitionedIntervalPair {
    pub(crate) fine_subinterval_count_per_coarse_interval: usize,
}

impl PartitionedIntervalPair {
    pub(crate) fn try_new(
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
    ) -> Result<Self, PartitionedIntervalPairError> {
        if !coarse_interval_delta.is_finite() || coarse_interval_delta <= 0.0 {
            return Err(PartitionedIntervalPairError::InvalidCoarseIntervalDelta {
                value: coarse_interval_delta,
            });
        }
        if !fine_interval_delta.is_finite() || fine_interval_delta <= 0.0 {
            return Err(PartitionedIntervalPairError::InvalidFineIntervalDelta {
                value: fine_interval_delta,
            });
        }
        if fine_interval_delta >= coarse_interval_delta {
            return Err(
                PartitionedIntervalPairError::FineIntervalMustBeStrictlySmaller {
                    coarse_interval_delta,
                    fine_interval_delta,
                },
            );
        }

        let implied_ratio = coarse_interval_delta / fine_interval_delta;
        let rounded_ratio = implied_ratio.round();
        if rounded_ratio < 2.0
            || (implied_ratio - rounded_ratio).abs() > PARTITIONED_INTERVAL_RATIO_TOLERANCE
        {
            return Err(PartitionedIntervalPairError::IntervalDeltaRatioMismatch {
                coarse_interval_delta,
                fine_interval_delta,
                implied_ratio,
            });
        }

        Ok(Self {
            fine_subinterval_count_per_coarse_interval: rounded_ratio as usize,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PartitionedIntervalPairError {
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
}

impl Display for PartitionedIntervalPairError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
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
        }
    }
}

impl Error for PartitionedIntervalPairError {}
