//! Timestamp observation-layer contracts shared by fitted models and simulators.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

/// Schema version for timestamp batching observation models.
pub const TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION: &str =
    "model.timestamp_batching_observation.v1";

/// Inference policy used when a fixed timestamp bucket can be detected from observed ticks.
pub const TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY: &str =
    "same_session_duplicate_timestamps_with_positive_timestamp_quantum_gcd.v1";

/// Source label for a fixed timestamp bucket inferred from training data.
pub const TIMESTAMP_BATCHING_INFERRED_BUCKET_SOURCE: &str =
    "training_data_inferred_same_session_timestamp_quantum";

/// Source label used when the batching layer is explicitly disabled or not detectable.
pub const TIMESTAMP_BATCHING_DISABLED_SOURCE: &str = "unavailable";

/// Observation policy for direct latent-time timestamp emission.
pub const TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY: &str =
    "latent_event_time_equals_observed_timestamp.v1";

/// Observation policy for fixed-width batching that floors trade events inside their latent interval.
pub const TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY: &str = "floor_latent_event_time_to_fixed_width_bucket_with_same_timestamp_continuation_within_latent_interval.v1";

/// Coarse-interval compatibility policy for observation mappings that preserve latent ownership.
pub const TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY: &str =
    "observed_timestamp_ownership_constrained_to_latent_interval_for_direct_coarse_consistency.v1";

/// Timestamp observation-layer mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampBatchingObservationMode {
    /// Emit the latent event time as the observed timestamp.
    #[default]
    Disabled,

    /// Floor eligible latent event times to a fixed-width timestamp bucket inside the latent interval.
    FixedBucketFloor,
}

impl TimestampBatchingObservationMode {
    /// Stable mode identifier used by diagnostics and machine-readable progress.
    pub const fn mode_id(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::FixedBucketFloor => "fixed_bucket_floor",
        }
    }
}

/// Fitted or requested timestamp batching observation model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimestampBatchingObservationModel {
    /// Contract schema version.
    pub schema_version: String,

    /// Observation mode.
    pub mode: TimestampBatchingObservationMode,

    /// Bucket width in UTC nanoseconds when `mode` is `fixed_bucket_floor`.
    pub bucket_width_nanos: Option<i64>,

    /// Same-timestamp adjacent event count used to fit observed batch continuation.
    pub same_timestamp_continuation_count: Option<u64>,

    /// Same-session adjacent event count used to normalize observed batch continuation.
    pub same_session_observation_transition_count: Option<u64>,

    /// Data or request source that supplied the model.
    pub bucket_width_source: String,

    /// Inference policy for the bucket width.
    pub inference_policy: String,

    /// Generative observation policy applied by simulators.
    pub observation_policy: String,

    /// Direct coarse interval compatibility policy.
    pub coarse_interval_compatibility_policy: String,
}

impl TimestampBatchingObservationModel {
    /// Build a disabled timestamp observation layer.
    pub fn disabled() -> Self {
        Self {
            schema_version: TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION.to_string(),
            mode: TimestampBatchingObservationMode::Disabled,
            bucket_width_nanos: None,
            same_timestamp_continuation_count: None,
            same_session_observation_transition_count: None,
            bucket_width_source: TIMESTAMP_BATCHING_DISABLED_SOURCE.to_string(),
            inference_policy: TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY.to_string(),
            observation_policy: TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY.to_string(),
            coarse_interval_compatibility_policy:
                TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY.to_string(),
        }
    }

    /// Build a fixed-bucket timestamp observation layer.
    pub fn fixed_bucket_floor(
        bucket_width_nanos: i64,
        bucket_width_source: impl Into<String>,
        same_timestamp_continuation_count: u64,
        same_session_observation_transition_count: u64,
    ) -> Result<Self, TimestampBatchingObservationModelError> {
        let model = Self {
            schema_version: TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION.to_string(),
            mode: TimestampBatchingObservationMode::FixedBucketFloor,
            bucket_width_nanos: Some(bucket_width_nanos),
            same_timestamp_continuation_count: Some(same_timestamp_continuation_count),
            same_session_observation_transition_count: Some(
                same_session_observation_transition_count,
            ),
            bucket_width_source: bucket_width_source.into(),
            inference_policy: TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY.to_string(),
            observation_policy: TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY.to_string(),
            coarse_interval_compatibility_policy:
                TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY.to_string(),
        };

        model.validate()?;

        Ok(model)
    }

    /// Validate the observation contract.
    pub fn validate(&self) -> Result<(), TimestampBatchingObservationModelError> {
        if self.schema_version != TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION {
            return Err(
                TimestampBatchingObservationModelError::InvalidSchemaVersion {
                    // Error variants own invalid values so callers can report them after validation returns.
                    value: self.schema_version.clone(),
                },
            );
        }

        validate_non_empty_policy_field("bucket_width_source", &self.bucket_width_source)?;
        validate_non_empty_policy_field("inference_policy", &self.inference_policy)?;
        validate_non_empty_policy_field("observation_policy", &self.observation_policy)?;

        validate_non_empty_policy_field(
            "coarse_interval_compatibility_policy",
            &self.coarse_interval_compatibility_policy,
        )?;

        if self.coarse_interval_compatibility_policy
            != TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY
        {
            return Err(
                TimestampBatchingObservationModelError::UnsupportedCoarseIntervalCompatibilityPolicy {
                    // Error variants own invalid values so callers can report them after validation returns.
                    value: self.coarse_interval_compatibility_policy.clone(),
                },
            );
        }

        match self.mode {
            TimestampBatchingObservationMode::Disabled => {
                if self.bucket_width_nanos.is_some() {
                    return Err(TimestampBatchingObservationModelError::DisabledModeHasBucketWidth);
                }

                if self.same_timestamp_continuation_count.is_some() {
                    return Err(
                        TimestampBatchingObservationModelError::DisabledModeHasContinuationCounts,
                    );
                }

                if self.same_session_observation_transition_count.is_some() {
                    return Err(
                        TimestampBatchingObservationModelError::DisabledModeHasContinuationCounts,
                    );
                }

                if self.observation_policy != TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY {
                    return Err(
                        TimestampBatchingObservationModelError::ModeObservationPolicyMismatch {
                            mode: self.mode,
                            // Error variants own invalid values so callers can report them after validation returns.
                            observation_policy: self.observation_policy.clone(),
                        },
                    );
                }
            }
            TimestampBatchingObservationMode::FixedBucketFloor => {
                let Some(bucket_width_nanos) = self.bucket_width_nanos else {
                    return Err(
                        TimestampBatchingObservationModelError::FixedBucketModeMissingBucketWidth,
                    );
                };

                if bucket_width_nanos <= 0 {
                    return Err(
                        TimestampBatchingObservationModelError::InvalidBucketWidthNanos {
                            value: bucket_width_nanos,
                        },
                    );
                }

                if self.observation_policy != TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY {
                    return Err(
                        TimestampBatchingObservationModelError::ModeObservationPolicyMismatch {
                            mode: self.mode,
                            // Error variants own invalid values so callers can report them after validation returns.
                            observation_policy: self.observation_policy.clone(),
                        },
                    );
                }

                let Some(same_timestamp_continuation_count) =
                    self.same_timestamp_continuation_count
                else {
                    return Err(
                        TimestampBatchingObservationModelError::FixedBucketModeMissingContinuationCounts,
                    );
                };

                let Some(same_session_observation_transition_count) =
                    self.same_session_observation_transition_count
                else {
                    return Err(
                        TimestampBatchingObservationModelError::FixedBucketModeMissingContinuationCounts,
                    );
                };

                if same_session_observation_transition_count == 0 {
                    return Err(
                        TimestampBatchingObservationModelError::InvalidContinuationCounts {
                            same_timestamp_continuation_count,
                            same_session_observation_transition_count,
                        },
                    );
                }

                if same_timestamp_continuation_count > same_session_observation_transition_count {
                    return Err(
                        TimestampBatchingObservationModelError::InvalidContinuationCounts {
                            same_timestamp_continuation_count,
                            same_session_observation_transition_count,
                        },
                    );
                }
            }
        }

        Ok(())
    }

    /// Probability that an endogenous event continues the previous observed timestamp batch.
    pub fn same_timestamp_continuation_probability(&self) -> Option<f64> {
        let continuation_count = self.same_timestamp_continuation_count?;
        let transition_count = self.same_session_observation_transition_count?;

        if transition_count == 0 {
            return None;
        }

        Some(continuation_count as f64 / transition_count as f64)
    }

    /// Map one transition time into its observed timestamp under this contract.
    pub fn observed_timestamp_for_transition(
        &self,
        latent_timestamp_utc_nanos: i64,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        apply_batching: bool,
    ) -> Result<i64, TimestampBatchingObservationModelError> {
        self.validate()?;

        if !apply_batching || self.mode == TimestampBatchingObservationMode::Disabled {
            return Ok(latent_timestamp_utc_nanos);
        }

        if interval_end_utc_nanos <= interval_start_utc_nanos {
            return Err(
                TimestampBatchingObservationModelError::InvalidIntervalBounds {
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            );
        }

        if latent_timestamp_utc_nanos < interval_start_utc_nanos
            || latent_timestamp_utc_nanos >= interval_end_utc_nanos
        {
            return Err(
                TimestampBatchingObservationModelError::LatentTimestampOutsideInterval {
                    latent_timestamp_utc_nanos,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            );
        }

        let bucket_width_nanos = self
            .bucket_width_nanos
            .ok_or(TimestampBatchingObservationModelError::FixedBucketModeMissingBucketWidth)?;

        let elapsed_nanos = latent_timestamp_utc_nanos - interval_start_utc_nanos;
        let bucket_offset_nanos = (elapsed_nanos / bucket_width_nanos) * bucket_width_nanos;

        interval_start_utc_nanos
            .checked_add(bucket_offset_nanos)
            .ok_or(
                TimestampBatchingObservationModelError::ObservedTimestampOverflow {
                    interval_start_utc_nanos,
                    bucket_offset_nanos,
                },
            )
    }
}

impl Default for TimestampBatchingObservationModel {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Timestamp batching observation model validation and mapping errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimestampBatchingObservationModelError {
    InvalidSchemaVersion {
        value: String,
    },
    EmptyPolicyField {
        field_name: &'static str,
    },
    UnsupportedCoarseIntervalCompatibilityPolicy {
        value: String,
    },
    DisabledModeHasBucketWidth,
    DisabledModeHasContinuationCounts,
    FixedBucketModeMissingBucketWidth,
    FixedBucketModeMissingContinuationCounts,
    InvalidBucketWidthNanos {
        value: i64,
    },
    InvalidContinuationCounts {
        same_timestamp_continuation_count: u64,
        same_session_observation_transition_count: u64,
    },
    ModeObservationPolicyMismatch {
        mode: TimestampBatchingObservationMode,
        observation_policy: String,
    },
    InvalidIntervalBounds {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    LatentTimestampOutsideInterval {
        latent_timestamp_utc_nanos: i64,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    ObservedTimestampOverflow {
        interval_start_utc_nanos: i64,
        bucket_offset_nanos: i64,
    },
}

impl Display for TimestampBatchingObservationModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { value } => write!(
                formatter,
                "timestamp batching observation model schema_version must be `{TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION}`, found `{value}`"
            ),
            Self::EmptyPolicyField { field_name } => {
                write!(formatter, "{field_name} must be a non-empty string")
            }
            Self::UnsupportedCoarseIntervalCompatibilityPolicy { value } => write!(
                formatter,
                "coarse_interval_compatibility_policy must be `{TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY}`, found `{value}`"
            ),
            Self::DisabledModeHasBucketWidth => write!(
                formatter,
                "disabled timestamp batching observation mode must not carry bucket_width_nanos"
            ),
            Self::DisabledModeHasContinuationCounts => write!(
                formatter,
                "disabled timestamp batching observation mode must not carry same-timestamp continuation counts"
            ),
            Self::FixedBucketModeMissingBucketWidth => write!(
                formatter,
                "fixed_bucket_floor timestamp batching observation mode requires bucket_width_nanos"
            ),
            Self::FixedBucketModeMissingContinuationCounts => write!(
                formatter,
                "fixed_bucket_floor timestamp batching observation mode requires same-timestamp continuation counts"
            ),
            Self::InvalidBucketWidthNanos { value } => write!(
                formatter,
                "timestamp batching bucket_width_nanos must be strictly positive, found {value}"
            ),
            Self::InvalidContinuationCounts {
                same_timestamp_continuation_count,
                same_session_observation_transition_count,
            } => write!(
                formatter,
                "same-timestamp continuation counts must satisfy 0 <= continuation_count <= transition_count and transition_count > 0, found continuation_count={same_timestamp_continuation_count} transition_count={same_session_observation_transition_count}"
            ),
            Self::ModeObservationPolicyMismatch {
                mode,
                observation_policy,
            } => write!(
                formatter,
                "timestamp batching observation_policy `{observation_policy}` is not compatible with mode `{mode:?}`"
            ),
            Self::InvalidIntervalBounds {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "timestamp batching interval bounds must be increasing, found start={interval_start_utc_nanos} end={interval_end_utc_nanos}"
            ),
            Self::LatentTimestampOutsideInterval {
                latent_timestamp_utc_nanos,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "latent timestamp {latent_timestamp_utc_nanos} must be inside interval [{interval_start_utc_nanos}, {interval_end_utc_nanos}) for timestamp batching"
            ),
            Self::ObservedTimestampOverflow {
                interval_start_utc_nanos,
                bucket_offset_nanos,
            } => write!(
                formatter,
                "timestamp batching observed timestamp overflow for interval_start_utc_nanos={interval_start_utc_nanos} bucket_offset_nanos={bucket_offset_nanos}"
            ),
        }
    }
}

impl Error for TimestampBatchingObservationModelError {}

fn validate_non_empty_policy_field(
    field_name: &'static str,
    value: &str,
) -> Result<(), TimestampBatchingObservationModelError> {
    if value.trim().is_empty() {
        return Err(TimestampBatchingObservationModelError::EmptyPolicyField { field_name });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TimestampBatchingObservationMode, TimestampBatchingObservationModel};

    #[test]
    fn fixed_bucket_floor_maps_distinct_latent_times_to_same_observed_timestamp() {
        let model = TimestampBatchingObservationModel::fixed_bucket_floor(100, "test", 2, 10)
            .expect("fixed bucket model should validate");

        let first_observed = model
            .observed_timestamp_for_transition(1_025, 1_000, 2_000, true)
            .expect("first timestamp should map");

        let second_observed = model
            .observed_timestamp_for_transition(1_075, 1_000, 2_000, true)
            .expect("second timestamp should map");

        assert_eq!(
            model.mode,
            TimestampBatchingObservationMode::FixedBucketFloor
        );
        assert_eq!(first_observed, 1_000);
        assert_eq!(second_observed, 1_000);
    }

    #[test]
    fn disabled_model_preserves_observed_timestamp() {
        let model = TimestampBatchingObservationModel::disabled();

        let observed = model
            .observed_timestamp_for_transition(1_025, 1_000, 2_000, true)
            .expect("disabled model should preserve timestamp");

        assert_eq!(observed, 1_025);
    }
}
