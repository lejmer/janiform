use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::time::{
    NANOS_PER_HOUR, ceil_to_hour_utc_nanos, duration_nanos_to_hours, floor_to_hour_utc_nanos,
};
use janiform_market_time::hourly_factors::HourlyFactorContractOutput;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};

use crate::runner::{
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingIntervalFactorsError,
};
use crate::thread_pool::{
    build_pricing_thread_pool, install_pricing_parallelism, resolved_pricing_cpu_count,
};

const AUTO_ACTIVITY_COMPRESSED_POLICY_ID: &str = "auto_activity_compressed.v1";
const AUTO_ACTIVITY_COMPRESSED_DERIVATION_POLICY: &str = "expiry_preserving_first_open_day_hourly_then_two_hour_open_compression_from_hourly_factor_coverage.v1";
const AUTO_ACTIVITY_COMPRESSED_FACTOR_ASSIGNMENT_POLICY: &str =
    "overlap_weighted_resampling_from_hourly_source_buckets.v1";
const AUTO_EXACT_SPARSE_POLICY_ID: &str = "auto_exact_sparse.v1";
const AUTO_EXACT_SPARSE_DERIVATION_POLICY: &str =
    "valuation_time_and_unique_expiries_union_utc_hour_boundaries.v1";
const AUTO_EXACT_SPARSE_FACTOR_ASSIGNMENT_POLICY: &str =
    "assign_containing_utc_hour_bucket_without_overlap_weighting.v1";
const EXPLICIT_INTERVAL_ENDPOINT_POLICY_ID: &str = "explicit_interval_end_timestamps_utc.v1";
const EXPLICIT_INTERVAL_ENDPOINT_DERIVATION_POLICY: &str =
    "explicit_user_supplied_interval_endpoints.v1";
const EXPLICIT_INTERVAL_ENDPOINT_FACTOR_ASSIGNMENT_POLICY: &str =
    "overlap_weighted_resampling_from_hourly_source_buckets.v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum PricingIntervalSchedulePolicy {
    #[default]
    AutoActivityCompressed,
    AutoExactSparse,
    ExplicitIntervalEndTimestampsUtc {
        interval_end_timestamps_utc_nanos: Vec<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricingIntervalScheduleMetadata {
    pub policy_id: String,
    pub derivation_policy: String,
    pub factor_assignment_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricingIntervalSchedule {
    pub valuation_time_utc_nanos: i64,
    pub metadata: PricingIntervalScheduleMetadata,
    pub interval_end_timestamps_utc_nanos: Vec<i64>,
    pub interval_deltas_hours: Vec<f64>,
}

impl PricingIntervalSchedule {
    pub fn try_new(
        valuation_time_utc_nanos: i64,
        metadata: PricingIntervalScheduleMetadata,
        interval_end_timestamps_utc_nanos: Vec<i64>,
    ) -> Result<Self, PricingIntervalScheduleError> {
        if interval_end_timestamps_utc_nanos.is_empty() {
            return Err(PricingIntervalScheduleError::EmptyIntervalEndpoints);
        }

        let mut interval_deltas_hours = Vec::with_capacity(interval_end_timestamps_utc_nanos.len());
        let mut previous_endpoint_utc_nanos = valuation_time_utc_nanos;

        for (interval_index, &interval_end_utc_nanos) in
            interval_end_timestamps_utc_nanos.iter().enumerate()
        {
            if interval_end_utc_nanos <= previous_endpoint_utc_nanos {
                return Err(
                    PricingIntervalScheduleError::NonIncreasingIntervalEndpoint {
                        interval_index,
                        previous_endpoint_utc_nanos,
                        current_endpoint_utc_nanos: interval_end_utc_nanos,
                    },
                );
            }

            interval_deltas_hours.push(duration_nanos_to_hours(
                interval_end_utc_nanos - previous_endpoint_utc_nanos,
            ));

            previous_endpoint_utc_nanos = interval_end_utc_nanos;
        }

        Ok(Self {
            valuation_time_utc_nanos,
            metadata,
            interval_end_timestamps_utc_nanos,
            interval_deltas_hours,
        })
    }

    pub fn interval_count(&self) -> usize {
        self.interval_end_timestamps_utc_nanos.len()
    }

    pub fn interval_start_utc_nanos(&self, interval_index: usize) -> Option<i64> {
        if interval_index >= self.interval_end_timestamps_utc_nanos.len() {
            return None;
        }

        Some(if interval_index == 0 {
            self.valuation_time_utc_nanos
        } else {
            self.interval_end_timestamps_utc_nanos[interval_index - 1]
        })
    }

    pub fn interval_end_utc_nanos(&self, interval_index: usize) -> Option<i64> {
        self.interval_end_timestamps_utc_nanos
            .get(interval_index)
            .copied()
    }

    pub fn interval_delta_hours(&self, interval_index: usize) -> Option<f64> {
        self.interval_deltas_hours.get(interval_index).copied()
    }

    pub fn terminal_interval_index_for_timestamp(
        &self,
        expiry_timestamp_utc_nanos: i64,
    ) -> Result<usize, PricingIntervalScheduleError> {
        self.interval_end_timestamps_utc_nanos
            .iter()
            .position(|&value| value == expiry_timestamp_utc_nanos)
            .ok_or(
                PricingIntervalScheduleError::ExpiryMissingTerminalBoundary {
                    expiry_timestamp_utc_nanos,
                },
            )
    }

    pub fn maturity_interval_hours(
        &self,
        terminal_interval_index: usize,
    ) -> Result<f64, PricingIntervalScheduleError> {
        let terminal_interval_end_utc_nanos = self
            .interval_end_utc_nanos(terminal_interval_index)
            .ok_or(PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                terminal_interval_index,
                interval_count: self.interval_count(),
            })?;

        Ok(duration_nanos_to_hours(
            terminal_interval_end_utc_nanos - self.valuation_time_utc_nanos,
        ))
    }

    pub fn terminal_interval_index_for_maturity_interval(
        &self,
        maturity_interval: f64,
    ) -> Result<usize, PricingIntervalScheduleError> {
        (0..self.interval_count())
            .find(|&terminal_interval_index| {
                self.maturity_interval_hours(terminal_interval_index)
                    .is_ok_and(|resolved_interval| resolved_interval == maturity_interval)
            })
            .ok_or(
                PricingIntervalScheduleError::MaturityIntervalMissingTerminalBoundary {
                    maturity_interval,
                },
            )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedPricingIntervalSchedule {
    pub schedule: PricingIntervalSchedule,
    pub maturity_terminal_interval_indices: Vec<usize>,
}

pub fn derive_pricing_interval_schedule(
    valuation_time_utc_nanos: i64,
    expiry_timestamps_utc_nanos: &[i64],
    policy: &PricingIntervalSchedulePolicy,
) -> Result<DerivedPricingIntervalSchedule, PricingIntervalScheduleError> {
    validate_expiry_axis(valuation_time_utc_nanos, expiry_timestamps_utc_nanos)?;

    let schedule = match policy {
        PricingIntervalSchedulePolicy::AutoActivityCompressed => {
            return Err(
                PricingIntervalScheduleError::HourlyFactorCoverageRequiredForPolicy {
                    policy_id: AUTO_ACTIVITY_COMPRESSED_POLICY_ID.to_string(),
                },
            );
        }
        PricingIntervalSchedulePolicy::AutoExactSparse => {
            build_auto_exact_sparse_schedule(valuation_time_utc_nanos, expiry_timestamps_utc_nanos)?
        }
        PricingIntervalSchedulePolicy::ExplicitIntervalEndTimestampsUtc {
            interval_end_timestamps_utc_nanos,
        } => build_explicit_interval_schedule(
            valuation_time_utc_nanos,
            interval_end_timestamps_utc_nanos.as_slice(),
        )?,
    };

    build_derived_schedule(schedule, expiry_timestamps_utc_nanos)
}

pub fn derive_activity_compressed_pricing_interval_schedule(
    valuation_time_utc_nanos: i64,
    expiry_timestamps_utc_nanos: &[i64],
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<DerivedPricingIntervalSchedule, PricingIntervalScheduleError> {
    validate_expiry_axis(valuation_time_utc_nanos, expiry_timestamps_utc_nanos)?;

    let schedule = build_auto_activity_compressed_schedule(
        valuation_time_utc_nanos,
        expiry_timestamps_utc_nanos,
        hourly_factor_contract,
    )?;

    build_derived_schedule(schedule, expiry_timestamps_utc_nanos)
}

pub fn materialize_schedule_interval_factors(
    schedule: &PricingIntervalSchedule,
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<Vec<VanillaOptionPricingIntervalFactors>, PricingIntervalScheduleError> {
    materialize_schedule_interval_factors_with_cpu_count(schedule, hourly_factor_contract, None)
}

pub fn materialize_schedule_interval_factors_with_cpu_count(
    schedule: &PricingIntervalSchedule,
    hourly_factor_contract: &HourlyFactorContractOutput,
    cpu_count: Option<usize>,
) -> Result<Vec<VanillaOptionPricingIntervalFactors>, PricingIntervalScheduleError> {
    if hourly_factor_contract.interval_duration_nanos != NANOS_PER_HOUR {
        return Err(
            PricingIntervalScheduleError::UnexpectedHourlyContractIntervalDuration {
                observed_interval_duration_nanos: hourly_factor_contract.interval_duration_nanos,
            },
        );
    }

    if hourly_factor_contract.interval_count() == 0 {
        return Err(PricingIntervalScheduleError::EmptyHourlyFactorContract);
    }

    if hourly_factor_contract.interval_start_utc_nanos.len()
        != hourly_factor_contract.interval_end_utc_nanos.len()
        || hourly_factor_contract.interval_start_utc_nanos.len() != hourly_factor_contract.s_h.len()
        || hourly_factor_contract.interval_start_utc_nanos.len()
            != hourly_factor_contract.known_component_nu_h.len()
        || hourly_factor_contract.interval_start_utc_nanos.len()
            != hourly_factor_contract.surprise_component_nu_h.len()
    {
        return Err(
            PricingIntervalScheduleError::HourlyFactorContractShapeMismatch {
                interval_count: hourly_factor_contract.interval_start_utc_nanos.len(),
                seasonality_factor_count: hourly_factor_contract.s_h.len(),
                known_news_factor_count: hourly_factor_contract.known_component_nu_h.len(),
                surprise_news_factor_count: hourly_factor_contract.surprise_component_nu_h.len(),
            },
        );
    }
    let first_contract_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];
    let last_contract_end_utc_nanos = hourly_factor_contract.interval_end_utc_nanos
        [hourly_factor_contract.interval_end_utc_nanos.len() - 1];

    if schedule.valuation_time_utc_nanos < first_contract_start_utc_nanos {
        return Err(
            PricingIntervalScheduleError::ScheduleStartsBeforeHourlyCoverage {
                schedule_start_utc_nanos: schedule.valuation_time_utc_nanos,
                hourly_coverage_start_utc_nanos: first_contract_start_utc_nanos,
            },
        );
    }

    let last_schedule_end_utc_nanos = schedule.interval_end_timestamps_utc_nanos
        [schedule.interval_end_timestamps_utc_nanos.len() - 1];
    if last_schedule_end_utc_nanos > last_contract_end_utc_nanos {
        return Err(
            PricingIntervalScheduleError::ScheduleEndsAfterHourlyCoverage {
                schedule_end_utc_nanos: last_schedule_end_utc_nanos,
                hourly_coverage_end_utc_nanos: last_contract_end_utc_nanos,
            },
        );
    }

    let factors = match schedule.metadata.policy_id.as_str() {
        AUTO_EXACT_SPARSE_POLICY_ID => schedule
            .interval_end_timestamps_utc_nanos
            .iter()
            .enumerate()
            .map(|(interval_index, &interval_end_utc_nanos)| {
                let interval_start_utc_nanos = schedule
                    .interval_start_utc_nanos(interval_index)
                    .ok_or(PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                        terminal_interval_index: interval_index,
                        interval_count: schedule.interval_count(),
                    })?;
                let hour_bucket_start_utc_nanos = floor_to_hour_utc_nanos(interval_start_utc_nanos);
                let hour_bucket_index = ((hour_bucket_start_utc_nanos
                    - first_contract_start_utc_nanos)
                    / NANOS_PER_HOUR) as usize;
                let contract_hour_end_utc_nanos =
                    hourly_factor_contract.interval_end_utc_nanos[hour_bucket_index];
                if interval_end_utc_nanos > contract_hour_end_utc_nanos {
                    return Err(
                        PricingIntervalScheduleError::AutomaticIntervalCrossesHourBoundary {
                            interval_index,
                            interval_start_utc_nanos,
                            interval_end_utc_nanos,
                            containing_hour_end_utc_nanos: contract_hour_end_utc_nanos,
                        },
                    );
                }

                build_pricing_interval_factors_from_hour_index(
                    hourly_factor_contract,
                    hour_bucket_index,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
        AUTO_ACTIVITY_COMPRESSED_POLICY_ID | EXPLICIT_INTERVAL_ENDPOINT_POLICY_ID => {
            let pricing_thread_pool = build_pricing_thread_pool(cpu_count);
            let resolved_cpu_count = resolved_pricing_cpu_count(cpu_count);
            let interval_count = schedule.interval_count();

            let interval_results = if resolved_cpu_count > 1 && interval_count > 1 {
                install_pricing_parallelism(pricing_thread_pool.as_ref(), || {
                    (0..interval_count)
                        .into_par_iter()
                        .map(|interval_index| {
                            let interval_start_utc_nanos =
                                schedule.interval_start_utc_nanos(interval_index).ok_or(
                                    PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                                        terminal_interval_index: interval_index,
                                        interval_count,
                                    },
                                )?;
                            let interval_end_utc_nanos =
                                schedule.interval_end_utc_nanos(interval_index).ok_or(
                                    PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                                        terminal_interval_index: interval_index,
                                        interval_count,
                                    },
                                )?;

                            build_resampled_pricing_interval_factors(
                                hourly_factor_contract,
                                interval_start_utc_nanos,
                                interval_end_utc_nanos,
                            )
                        })
                        .collect::<Vec<_>>()
                })
            } else {
                (0..interval_count)
                    .map(|interval_index| {
                        let interval_start_utc_nanos = schedule
                            .interval_start_utc_nanos(interval_index)
                            .ok_or(PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                                terminal_interval_index: interval_index,
                                interval_count,
                            })?;
                        let interval_end_utc_nanos = schedule
                            .interval_end_utc_nanos(interval_index)
                            .ok_or(PricingIntervalScheduleError::InvalidTerminalIntervalIndex {
                                terminal_interval_index: interval_index,
                                interval_count,
                            })?;

                        build_resampled_pricing_interval_factors(
                            hourly_factor_contract,
                            interval_start_utc_nanos,
                            interval_end_utc_nanos,
                        )
                    })
                    .collect::<Vec<_>>()
            };

            interval_results
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?
        }
        policy_id => {
            return Err(
                PricingIntervalScheduleError::UnsupportedFactorAssignmentPolicy {
                    policy_id: policy_id.to_string(),
                },
            );
        }
    };

    Ok(factors)
}

fn build_auto_exact_sparse_schedule(
    valuation_time_utc_nanos: i64,
    expiry_timestamps_utc_nanos: &[i64],
) -> Result<PricingIntervalSchedule, PricingIntervalScheduleError> {
    let mut endpoints = BTreeSet::<i64>::new();

    let max_expiry_timestamp_utc_nanos = expiry_timestamps_utc_nanos
        .iter()
        .copied()
        .max()
        .ok_or(PricingIntervalScheduleError::EmptyExpiryAxis)?;

    let first_hour_boundary_utc_nanos = ceil_to_hour_utc_nanos(valuation_time_utc_nanos);
    let final_hour_boundary_utc_nanos = floor_to_hour_utc_nanos(max_expiry_timestamp_utc_nanos);

    let mut current_hour_boundary_utc_nanos = first_hour_boundary_utc_nanos;
    while current_hour_boundary_utc_nanos <= final_hour_boundary_utc_nanos {
        if current_hour_boundary_utc_nanos > valuation_time_utc_nanos
            && current_hour_boundary_utc_nanos < max_expiry_timestamp_utc_nanos
        {
            endpoints.insert(current_hour_boundary_utc_nanos);
        }

        current_hour_boundary_utc_nanos = current_hour_boundary_utc_nanos
            .checked_add(NANOS_PER_HOUR)
            .ok_or(PricingIntervalScheduleError::AutomaticScheduleOverflow)?;
    }

    for &expiry_timestamp_utc_nanos in expiry_timestamps_utc_nanos {
        endpoints.insert(expiry_timestamp_utc_nanos);
    }

    PricingIntervalSchedule::try_new(
        valuation_time_utc_nanos,
        PricingIntervalScheduleMetadata {
            policy_id: AUTO_EXACT_SPARSE_POLICY_ID.to_string(),
            derivation_policy: AUTO_EXACT_SPARSE_DERIVATION_POLICY.to_string(),
            factor_assignment_policy: AUTO_EXACT_SPARSE_FACTOR_ASSIGNMENT_POLICY.to_string(),
        },
        endpoints.into_iter().collect(),
    )
}

fn build_auto_activity_compressed_schedule(
    valuation_time_utc_nanos: i64,
    expiry_timestamps_utc_nanos: &[i64],
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<PricingIntervalSchedule, PricingIntervalScheduleError> {
    if hourly_factor_contract.interval_duration_nanos != NANOS_PER_HOUR {
        return Err(
            PricingIntervalScheduleError::UnexpectedHourlyContractIntervalDuration {
                observed_interval_duration_nanos: hourly_factor_contract.interval_duration_nanos,
            },
        );
    }

    if hourly_factor_contract.interval_count() == 0 {
        return Err(PricingIntervalScheduleError::EmptyHourlyFactorContract);
    }

    let first_contract_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];
    if valuation_time_utc_nanos < first_contract_start_utc_nanos {
        return Err(
            PricingIntervalScheduleError::ScheduleStartsBeforeHourlyCoverage {
                schedule_start_utc_nanos: valuation_time_utc_nanos,
                hourly_coverage_start_utc_nanos: first_contract_start_utc_nanos,
            },
        );
    }

    let max_expiry_timestamp_utc_nanos = expiry_timestamps_utc_nanos
        .iter()
        .copied()
        .max()
        .ok_or(PricingIntervalScheduleError::EmptyExpiryAxis)?;
    let last_contract_end_utc_nanos = hourly_factor_contract.interval_end_utc_nanos
        [hourly_factor_contract.interval_end_utc_nanos.len() - 1];
    if max_expiry_timestamp_utc_nanos > last_contract_end_utc_nanos {
        return Err(
            PricingIntervalScheduleError::ScheduleEndsAfterHourlyCoverage {
                schedule_end_utc_nanos: max_expiry_timestamp_utc_nanos,
                hourly_coverage_end_utc_nanos: last_contract_end_utc_nanos,
            },
        );
    }

    let mut unique_expiry_endpoints = expiry_timestamps_utc_nanos.to_vec();
    unique_expiry_endpoints.sort_unstable();
    unique_expiry_endpoints.dedup();

    let mut expiry_cursor = 0usize;
    let mut interval_endpoints = Vec::new();
    let mut current_start_utc_nanos = valuation_time_utc_nanos;
    let mut elapsed_open_exposure_hours = 0.0;

    while current_start_utc_nanos < max_expiry_timestamp_utc_nanos {
        let current_hour_index =
            locate_hour_bucket_index(hourly_factor_contract, current_start_utc_nanos)?;
        let current_state_is_open =
            hour_bucket_has_open_exposure(hourly_factor_contract, current_hour_index);
        let next_state_transition_utc_nanos =
            next_state_transition_utc_nanos(hourly_factor_contract, current_hour_index);
        let next_expiry_utc_nanos = next_expiry_after(
            unique_expiry_endpoints.as_slice(),
            &mut expiry_cursor,
            current_start_utc_nanos,
        )
        .unwrap_or(max_expiry_timestamp_utc_nanos);

        let mut interval_end_utc_nanos = if current_state_is_open {
            let target_open_exposure_hours =
                target_open_exposure_quantum_hours(elapsed_open_exposure_hours);
            let target_end_utc_nanos = current_start_utc_nanos
                .checked_add(
                    (target_open_exposure_hours.round() as i64)
                        .checked_mul(NANOS_PER_HOUR)
                        .ok_or(PricingIntervalScheduleError::AutomaticScheduleOverflow)?,
                )
                .ok_or(PricingIntervalScheduleError::AutomaticScheduleOverflow)?;

            min_defined_boundary(
                target_end_utc_nanos,
                next_state_transition_utc_nanos,
                Some(next_expiry_utc_nanos),
                max_expiry_timestamp_utc_nanos,
            )
        } else {
            min_defined_boundary(
                next_state_transition_utc_nanos.unwrap_or(max_expiry_timestamp_utc_nanos),
                None,
                Some(next_expiry_utc_nanos),
                max_expiry_timestamp_utc_nanos,
            )
        };

        if interval_end_utc_nanos <= current_start_utc_nanos {
            interval_end_utc_nanos = next_expiry_utc_nanos;
        }

        if interval_end_utc_nanos <= current_start_utc_nanos {
            return Err(PricingIntervalScheduleError::NonPositiveExplicitInterval {
                interval_start_utc_nanos: current_start_utc_nanos,
                interval_end_utc_nanos,
            });
        }

        if current_state_is_open {
            elapsed_open_exposure_hours +=
                duration_nanos_to_hours(interval_end_utc_nanos - current_start_utc_nanos);
        }

        interval_endpoints.push(interval_end_utc_nanos);
        current_start_utc_nanos = interval_end_utc_nanos;
    }

    PricingIntervalSchedule::try_new(
        valuation_time_utc_nanos,
        PricingIntervalScheduleMetadata {
            policy_id: AUTO_ACTIVITY_COMPRESSED_POLICY_ID.to_string(),
            derivation_policy: AUTO_ACTIVITY_COMPRESSED_DERIVATION_POLICY.to_string(),
            factor_assignment_policy: AUTO_ACTIVITY_COMPRESSED_FACTOR_ASSIGNMENT_POLICY.to_string(),
        },
        interval_endpoints,
    )
}

fn build_explicit_interval_schedule(
    valuation_time_utc_nanos: i64,
    interval_end_timestamps_utc_nanos: &[i64],
) -> Result<PricingIntervalSchedule, PricingIntervalScheduleError> {
    PricingIntervalSchedule::try_new(
        valuation_time_utc_nanos,
        PricingIntervalScheduleMetadata {
            policy_id: EXPLICIT_INTERVAL_ENDPOINT_POLICY_ID.to_string(),
            derivation_policy: EXPLICIT_INTERVAL_ENDPOINT_DERIVATION_POLICY.to_string(),
            factor_assignment_policy: EXPLICIT_INTERVAL_ENDPOINT_FACTOR_ASSIGNMENT_POLICY
                .to_string(),
        },
        interval_end_timestamps_utc_nanos.to_vec(),
    )
}

fn validate_expiry_axis(
    valuation_time_utc_nanos: i64,
    expiry_timestamps_utc_nanos: &[i64],
) -> Result<(), PricingIntervalScheduleError> {
    if expiry_timestamps_utc_nanos.is_empty() {
        return Err(PricingIntervalScheduleError::EmptyExpiryAxis);
    }

    for (expiry_index, &expiry_timestamp_utc_nanos) in
        expiry_timestamps_utc_nanos.iter().enumerate()
    {
        if expiry_timestamp_utc_nanos <= valuation_time_utc_nanos {
            return Err(PricingIntervalScheduleError::ExpiryNotAfterValuation {
                expiry_index,
                valuation_time_utc_nanos,
                expiry_timestamp_utc_nanos,
            });
        }
    }

    Ok(())
}

fn build_derived_schedule(
    schedule: PricingIntervalSchedule,
    expiry_timestamps_utc_nanos: &[i64],
) -> Result<DerivedPricingIntervalSchedule, PricingIntervalScheduleError> {
    let maturity_terminal_interval_indices = expiry_timestamps_utc_nanos
        .iter()
        .copied()
        .map(|expiry_timestamp_utc_nanos| {
            schedule.terminal_interval_index_for_timestamp(expiry_timestamp_utc_nanos)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DerivedPricingIntervalSchedule {
        schedule,
        maturity_terminal_interval_indices,
    })
}

fn locate_hour_bucket_index(
    hourly_factor_contract: &HourlyFactorContractOutput,
    timestamp_utc_nanos: i64,
) -> Result<usize, PricingIntervalScheduleError> {
    let first_contract_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];
    let hour_bucket_index =
        ((timestamp_utc_nanos - first_contract_start_utc_nanos) / NANOS_PER_HOUR) as usize;

    if hour_bucket_index >= hourly_factor_contract.interval_count() {
        return Err(
            PricingIntervalScheduleError::ScheduleEndsAfterHourlyCoverage {
                schedule_end_utc_nanos: timestamp_utc_nanos,
                hourly_coverage_end_utc_nanos: hourly_factor_contract.interval_end_utc_nanos
                    [hourly_factor_contract.interval_end_utc_nanos.len() - 1],
            },
        );
    }

    Ok(hour_bucket_index)
}

fn hour_bucket_has_open_exposure(
    hourly_factor_contract: &HourlyFactorContractOutput,
    hour_bucket_index: usize,
) -> bool {
    hourly_factor_contract.s_h[hour_bucket_index] > 0.0
}

fn next_state_transition_utc_nanos(
    hourly_factor_contract: &HourlyFactorContractOutput,
    current_hour_index: usize,
) -> Option<i64> {
    let current_state_is_open =
        hour_bucket_has_open_exposure(hourly_factor_contract, current_hour_index);

    for next_hour_index in (current_hour_index + 1)..hourly_factor_contract.interval_count() {
        if hour_bucket_has_open_exposure(hourly_factor_contract, next_hour_index)
            != current_state_is_open
        {
            return Some(hourly_factor_contract.interval_start_utc_nanos[next_hour_index]);
        }
    }

    None
}

fn next_expiry_after(
    expiry_timestamps_utc_nanos: &[i64],
    expiry_cursor: &mut usize,
    current_start_utc_nanos: i64,
) -> Option<i64> {
    while *expiry_cursor < expiry_timestamps_utc_nanos.len()
        && expiry_timestamps_utc_nanos[*expiry_cursor] <= current_start_utc_nanos
    {
        *expiry_cursor += 1;
    }

    expiry_timestamps_utc_nanos.get(*expiry_cursor).copied()
}

fn target_open_exposure_quantum_hours(elapsed_open_exposure_hours: f64) -> f64 {
    if elapsed_open_exposure_hours < 24.0 {
        1.0
    } else {
        2.0
    }
}

fn min_defined_boundary(
    primary_boundary_utc_nanos: i64,
    secondary_boundary_utc_nanos: Option<i64>,
    tertiary_boundary_utc_nanos: Option<i64>,
    max_expiry_timestamp_utc_nanos: i64,
) -> i64 {
    let mut resolved_boundary_utc_nanos =
        primary_boundary_utc_nanos.min(max_expiry_timestamp_utc_nanos);

    if let Some(secondary_boundary_utc_nanos) = secondary_boundary_utc_nanos {
        resolved_boundary_utc_nanos = resolved_boundary_utc_nanos.min(secondary_boundary_utc_nanos);
    }

    if let Some(tertiary_boundary_utc_nanos) = tertiary_boundary_utc_nanos {
        resolved_boundary_utc_nanos = resolved_boundary_utc_nanos.min(tertiary_boundary_utc_nanos);
    }

    resolved_boundary_utc_nanos
}

fn build_pricing_interval_factors_from_hour_index(
    hourly_factor_contract: &HourlyFactorContractOutput,
    hour_bucket_index: usize,
) -> Result<VanillaOptionPricingIntervalFactors, PricingIntervalScheduleError> {
    VanillaOptionPricingIntervalFactors::try_new(
        hourly_factor_contract.s_h[hour_bucket_index],
        hourly_factor_contract.known_component_nu_h[hour_bucket_index],
        hourly_factor_contract.surprise_component_nu_h[hour_bucket_index],
    )
    .map_err(|source| PricingIntervalScheduleError::InvalidIntervalFactors { source })
}

fn build_resampled_pricing_interval_factors(
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<VanillaOptionPricingIntervalFactors, PricingIntervalScheduleError> {
    let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;
    if interval_duration_nanos <= 0 {
        return Err(PricingIntervalScheduleError::NonPositiveExplicitInterval {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        });
    }

    let first_contract_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];

    let first_hour_index =
        ((interval_start_utc_nanos - first_contract_start_utc_nanos) / NANOS_PER_HOUR) as usize;
    let last_hour_index =
        ((interval_end_utc_nanos - 1 - first_contract_start_utc_nanos) / NANOS_PER_HOUR) as usize;

    let mut seasonality_factor = 0.0;
    let mut known_news_factor = 0.0;
    let mut surprise_news_factor = 0.0;

    for hour_index in first_hour_index..=last_hour_index {
        let overlap_start_utc_nanos = interval_start_utc_nanos
            .max(hourly_factor_contract.interval_start_utc_nanos[hour_index]);
        let overlap_end_utc_nanos =
            interval_end_utc_nanos.min(hourly_factor_contract.interval_end_utc_nanos[hour_index]);
        let overlap_duration_nanos = overlap_end_utc_nanos - overlap_start_utc_nanos;
        if overlap_duration_nanos <= 0 {
            continue;
        }

        let overlap_weight = overlap_duration_nanos as f64 / interval_duration_nanos as f64;

        seasonality_factor += hourly_factor_contract.s_h[hour_index] * overlap_weight;
        known_news_factor +=
            hourly_factor_contract.known_component_nu_h[hour_index] * overlap_weight;
        surprise_news_factor +=
            hourly_factor_contract.surprise_component_nu_h[hour_index] * overlap_weight;
    }

    VanillaOptionPricingIntervalFactors::try_new(
        seasonality_factor,
        known_news_factor,
        surprise_news_factor,
    )
    .map_err(|source| PricingIntervalScheduleError::InvalidIntervalFactors { source })
}

#[derive(Debug, Clone, PartialEq)]
pub enum PricingIntervalScheduleError {
    EmptyExpiryAxis,
    EmptyIntervalEndpoints,
    ExpiryNotAfterValuation {
        expiry_index: usize,
        valuation_time_utc_nanos: i64,
        expiry_timestamp_utc_nanos: i64,
    },
    NonIncreasingIntervalEndpoint {
        interval_index: usize,
        previous_endpoint_utc_nanos: i64,
        current_endpoint_utc_nanos: i64,
    },
    ExpiryMissingTerminalBoundary {
        expiry_timestamp_utc_nanos: i64,
    },
    MaturityIntervalMissingTerminalBoundary {
        maturity_interval: f64,
    },
    InvalidTerminalIntervalIndex {
        terminal_interval_index: usize,
        interval_count: usize,
    },
    AutomaticScheduleOverflow,
    HourlyFactorCoverageRequiredForPolicy {
        policy_id: String,
    },
    UnexpectedHourlyContractIntervalDuration {
        observed_interval_duration_nanos: i64,
    },
    EmptyHourlyFactorContract,
    HourlyFactorContractShapeMismatch {
        interval_count: usize,
        seasonality_factor_count: usize,
        known_news_factor_count: usize,
        surprise_news_factor_count: usize,
    },
    ScheduleStartsBeforeHourlyCoverage {
        schedule_start_utc_nanos: i64,
        hourly_coverage_start_utc_nanos: i64,
    },
    ScheduleEndsAfterHourlyCoverage {
        schedule_end_utc_nanos: i64,
        hourly_coverage_end_utc_nanos: i64,
    },
    AutomaticIntervalCrossesHourBoundary {
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        containing_hour_end_utc_nanos: i64,
    },
    NonPositiveExplicitInterval {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    UnsupportedFactorAssignmentPolicy {
        policy_id: String,
    },
    InvalidIntervalFactors {
        source: VanillaOptionPricingIntervalFactorsError,
    },
}

impl Display for PricingIntervalScheduleError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyExpiryAxis => write!(
                formatter,
                "pricing interval schedule requires at least one expiry timestamp"
            ),
            Self::EmptyIntervalEndpoints => write!(
                formatter,
                "pricing interval schedule requires at least one interval endpoint"
            ),
            Self::ExpiryNotAfterValuation {
                expiry_index,
                valuation_time_utc_nanos,
                expiry_timestamp_utc_nanos,
            } => write!(
                formatter,
                "expiry_timestamps_utc_nanos[{expiry_index}]={expiry_timestamp_utc_nanos} must be strictly after valuation_time_utc_nanos={valuation_time_utc_nanos}"
            ),
            Self::NonIncreasingIntervalEndpoint {
                interval_index,
                previous_endpoint_utc_nanos,
                current_endpoint_utc_nanos,
            } => write!(
                formatter,
                "interval_end_timestamps_utc_nanos[{interval_index}]={current_endpoint_utc_nanos} must be strictly greater than the previous endpoint {previous_endpoint_utc_nanos}"
            ),
            Self::ExpiryMissingTerminalBoundary {
                expiry_timestamp_utc_nanos,
            } => write!(
                formatter,
                "expiry timestamp {expiry_timestamp_utc_nanos} must match a schedule terminal boundary exactly"
            ),
            Self::MaturityIntervalMissingTerminalBoundary { maturity_interval } => write!(
                formatter,
                "maturity_interval={maturity_interval} must match a schedule terminal boundary exactly"
            ),
            Self::InvalidTerminalIntervalIndex {
                terminal_interval_index,
                interval_count,
            } => write!(
                formatter,
                "terminal_interval_index={terminal_interval_index} must be smaller than interval_count={interval_count}"
            ),
            Self::AutomaticScheduleOverflow => write!(
                formatter,
                "automatic pricing interval schedule overflowed while advancing UTC hour boundaries"
            ),
            Self::HourlyFactorCoverageRequiredForPolicy { policy_id } => write!(
                formatter,
                "pricing interval schedule policy `{policy_id}` requires hourly factor coverage during schedule derivation"
            ),
            Self::UnexpectedHourlyContractIntervalDuration {
                observed_interval_duration_nanos,
            } => write!(
                formatter,
                "hourly factor contract must use exact UTC hourly source intervals, found interval_duration_nanos={observed_interval_duration_nanos}"
            ),
            Self::EmptyHourlyFactorContract => write!(
                formatter,
                "hourly factor contract must contain at least one interval"
            ),
            Self::HourlyFactorContractShapeMismatch {
                interval_count,
                seasonality_factor_count,
                known_news_factor_count,
                surprise_news_factor_count,
            } => write!(
                formatter,
                "hourly factor contract arrays must have identical lengths, found intervals={interval_count} seasonality={seasonality_factor_count} known_news={known_news_factor_count} surprise_news={surprise_news_factor_count}"
            ),
            Self::ScheduleStartsBeforeHourlyCoverage {
                schedule_start_utc_nanos,
                hourly_coverage_start_utc_nanos,
            } => write!(
                formatter,
                "pricing interval schedule starts at {schedule_start_utc_nanos} before hourly factor coverage start {hourly_coverage_start_utc_nanos}"
            ),
            Self::ScheduleEndsAfterHourlyCoverage {
                schedule_end_utc_nanos,
                hourly_coverage_end_utc_nanos,
            } => write!(
                formatter,
                "pricing interval schedule ends at {schedule_end_utc_nanos} after hourly factor coverage end {hourly_coverage_end_utc_nanos}"
            ),
            Self::AutomaticIntervalCrossesHourBoundary {
                interval_index,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
                containing_hour_end_utc_nanos,
            } => write!(
                formatter,
                "automatic sparse schedule interval {interval_index}=[{interval_start_utc_nanos}, {interval_end_utc_nanos}) crosses its containing UTC hour end {containing_hour_end_utc_nanos}"
            ),
            Self::NonPositiveExplicitInterval {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "explicit schedule interval [{interval_start_utc_nanos}, {interval_end_utc_nanos}) must be strictly positive"
            ),
            Self::UnsupportedFactorAssignmentPolicy { policy_id } => write!(
                formatter,
                "unsupported pricing interval factor assignment policy `{policy_id}`"
            ),
            Self::InvalidIntervalFactors { source } => {
                write!(formatter, "pricing interval factors are invalid: {source}")
            }
        }
    }
}

impl Error for PricingIntervalScheduleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

trait HourlyFactorContractCount {
    fn interval_count(&self) -> usize;
}

impl HourlyFactorContractCount for HourlyFactorContractOutput {
    fn interval_count(&self) -> usize {
        self.interval_start_utc_nanos.len()
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_HOUR, utc_datetime_from_nanos, utc_nanos};
    use janiform_market_time::hourly_factors::{
        HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HourlyFactorContractDiagnostics,
        HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
    };

    use super::{
        PricingIntervalSchedule, PricingIntervalScheduleError, PricingIntervalSchedulePolicy,
        derive_activity_compressed_pricing_interval_schedule, derive_pricing_interval_schedule,
        materialize_schedule_interval_factors,
    };

    fn build_hourly_factor_contract() -> HourlyFactorContractOutput {
        HourlyFactorContractOutput {
            schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
            exchange_code: "XCME".to_string(),
            exchange_timezone_name: "UTC".to_string(),
            request_start_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
            request_end_utc_nanos: utc_nanos(2026, 2, 2, 18, 0, 0),
            interval_duration_nanos: NANOS_PER_HOUR,
            interval_indices: vec![0, 1, 2, 3],
            interval_start_utc_nanos: vec![
                utc_nanos(2026, 2, 2, 14, 0, 0),
                utc_nanos(2026, 2, 2, 15, 0, 0),
                utc_nanos(2026, 2, 2, 16, 0, 0),
                utc_nanos(2026, 2, 2, 17, 0, 0),
            ],
            interval_end_utc_nanos: vec![
                utc_nanos(2026, 2, 2, 15, 0, 0),
                utc_nanos(2026, 2, 2, 16, 0, 0),
                utc_nanos(2026, 2, 2, 17, 0, 0),
                utc_nanos(2026, 2, 2, 18, 0, 0),
            ],
            interval_start_exchange_local: vec![
                "2026-02-02T14:00:00+00:00".to_string(),
                "2026-02-02T15:00:00+00:00".to_string(),
                "2026-02-02T16:00:00+00:00".to_string(),
                "2026-02-02T17:00:00+00:00".to_string(),
            ],
            interval_end_exchange_local: vec![
                "2026-02-02T15:00:00+00:00".to_string(),
                "2026-02-02T16:00:00+00:00".to_string(),
                "2026-02-02T17:00:00+00:00".to_string(),
                "2026-02-02T18:00:00+00:00".to_string(),
            ],
            s_h: vec![1.0, 2.0, 3.0, 4.0],
            known_component_nu_h: vec![10.0, 20.0, 30.0, 40.0],
            surprise_component_nu_h: vec![100.0, 200.0, 300.0, 400.0],
            nu_h: vec![110.0, 220.0, 330.0, 440.0],
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: "explicit".to_string(),
                news_intensity_materialization_method: "explicit".to_string(),
                interval_boundary_policy: "x",
                interval_alignment_policy: "x",
                timezone_transition_policy: "x",
                spring_forward_policy: "x",
                fall_back_policy: "x",
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count: 4,
                interval_duration_nanos: NANOS_PER_HOUR,
                first_interval_start_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                last_interval_end_utc_nanos: utc_nanos(2026, 2, 2, 18, 0, 0),
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        }
    }

    fn build_open_closed_hourly_factor_contract(
        request_start_utc_nanos: i64,
        open_hour_flags: &[bool],
    ) -> HourlyFactorContractOutput {
        let interval_count = open_hour_flags.len();
        let interval_start_utc_nanos = (0..interval_count)
            .map(|interval_index| request_start_utc_nanos + interval_index as i64 * NANOS_PER_HOUR)
            .collect::<Vec<_>>();
        let interval_end_utc_nanos = interval_start_utc_nanos
            .iter()
            .map(|interval_start_utc_nanos| interval_start_utc_nanos + NANOS_PER_HOUR)
            .collect::<Vec<_>>();
        let last_interval_end_utc_nanos = interval_end_utc_nanos[interval_end_utc_nanos.len() - 1];
        let s_h = open_hour_flags
            .iter()
            .map(|is_open| if *is_open { 1.0 } else { 0.0 })
            .collect::<Vec<_>>();
        let interval_start_exchange_local =
            exchange_local_labels(interval_start_utc_nanos.as_slice());
        let interval_end_exchange_local = exchange_local_labels(interval_end_utc_nanos.as_slice());

        HourlyFactorContractOutput {
            schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
            exchange_code: "XCME".to_string(),
            exchange_timezone_name: "UTC".to_string(),
            request_start_utc_nanos,
            request_end_utc_nanos: last_interval_end_utc_nanos,
            interval_duration_nanos: NANOS_PER_HOUR,
            interval_indices: (0..interval_count as u64).collect(),
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            interval_start_exchange_local,
            interval_end_exchange_local,
            s_h,
            known_component_nu_h: vec![0.0; interval_count],
            surprise_component_nu_h: vec![0.0; interval_count],
            nu_h: vec![0.0; interval_count],
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: "explicit".to_string(),
                news_intensity_materialization_method: "explicit".to_string(),
                interval_boundary_policy: "x",
                interval_alignment_policy: "x",
                timezone_transition_policy: "x",
                spring_forward_policy: "x",
                fall_back_policy: "x",
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count,
                interval_duration_nanos: NANOS_PER_HOUR,
                first_interval_start_utc_nanos: request_start_utc_nanos,
                last_interval_end_utc_nanos,
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        }
    }

    fn exchange_local_labels(timestamps_utc_nanos: &[i64]) -> Vec<String> {
        timestamps_utc_nanos
            .iter()
            .map(|timestamp_utc_nanos| {
                utc_datetime_from_nanos(*timestamp_utc_nanos)
                    .expect("test timestamp should be representable")
                    .to_rfc3339()
            })
            .collect()
    }

    #[test]
    fn auto_exact_sparse_schedule_includes_hour_boundaries_and_expiries() {
        let derived = derive_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 30, 0),
            &[
                utc_nanos(2026, 2, 2, 15, 15, 0),
                utc_nanos(2026, 2, 2, 17, 0, 0),
            ],
            &PricingIntervalSchedulePolicy::AutoExactSparse,
        )
        .expect("automatic schedule should resolve");

        assert_eq!(
            derived.schedule.interval_end_timestamps_utc_nanos,
            vec![
                utc_nanos(2026, 2, 2, 15, 0, 0),
                utc_nanos(2026, 2, 2, 15, 15, 0),
                utc_nanos(2026, 2, 2, 16, 0, 0),
                utc_nanos(2026, 2, 2, 17, 0, 0),
            ]
        );
        assert_eq!(derived.maturity_terminal_interval_indices, vec![1, 3]);
        assert_eq!(
            derived.schedule.interval_deltas_hours,
            vec![0.5, 0.25, 0.75, 1.0]
        );
    }

    #[test]
    fn explicit_schedule_requires_expiry_terminal_boundaries() {
        let error = derive_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 30, 0),
            &[utc_nanos(2026, 2, 2, 15, 15, 0)],
            &PricingIntervalSchedulePolicy::ExplicitIntervalEndTimestampsUtc {
                interval_end_timestamps_utc_nanos: vec![utc_nanos(2026, 2, 2, 15, 0, 0)],
            },
        )
        .expect_err("explicit schedule should reject unmatched expiry");

        assert_eq!(
            error,
            PricingIntervalScheduleError::ExpiryMissingTerminalBoundary {
                expiry_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 15, 15, 0)
            }
        );
    }

    #[test]
    fn automatic_factor_materialization_uses_containing_hour_bucket() {
        let hourly_factor_contract = build_hourly_factor_contract();
        let derived = derive_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 30, 0),
            &[utc_nanos(2026, 2, 2, 15, 15, 0)],
            &PricingIntervalSchedulePolicy::AutoExactSparse,
        )
        .expect("automatic schedule should resolve");

        let interval_factors =
            materialize_schedule_interval_factors(&derived.schedule, &hourly_factor_contract)
                .expect("automatic schedule factors should materialize");

        assert_eq!(interval_factors.len(), 2);
        assert_eq!(interval_factors[0].s_h, 1.0);
        assert_eq!(interval_factors[0].nu_h_known, 10.0);
        assert_eq!(interval_factors[0].nu_h_surprise, 100.0);
        assert_eq!(interval_factors[1].s_h, 2.0);
        assert_eq!(interval_factors[1].nu_h_known, 20.0);
        assert_eq!(interval_factors[1].nu_h_surprise, 200.0);
    }

    #[test]
    fn explicit_factor_materialization_resamples_overlap_weighted_hourly_sources() {
        let hourly_factor_contract = build_hourly_factor_contract();

        let schedule = PricingIntervalSchedule::try_new(
            utc_nanos(2026, 2, 2, 14, 30, 0),
            super::PricingIntervalScheduleMetadata {
                policy_id: "explicit_interval_end_timestamps_utc.v1".to_string(),
                derivation_policy: "explicit".to_string(),
                factor_assignment_policy:
                    "overlap_weighted_resampling_from_hourly_source_buckets.v1".to_string(),
            },
            vec![utc_nanos(2026, 2, 2, 16, 30, 0)],
        )
        .expect("explicit schedule should validate");

        let interval_factors =
            materialize_schedule_interval_factors(&schedule, &hourly_factor_contract)
                .expect("explicit schedule factors should materialize");

        assert_eq!(interval_factors.len(), 1);
        assert_eq!(interval_factors[0].s_h, 2.0);
        assert_eq!(interval_factors[0].nu_h_known, 20.0);
        assert_eq!(interval_factors[0].nu_h_surprise, 200.0);
    }

    #[test]
    fn activity_compressed_schedule_keeps_first_open_day_hourly_then_uses_two_hour_buckets() {
        let hourly_factor_contract =
            build_open_closed_hourly_factor_contract(utc_nanos(2026, 2, 2, 14, 0, 0), &[true; 28]);

        let derived = derive_activity_compressed_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 0, 0),
            &[utc_nanos(2026, 2, 3, 18, 0, 0)],
            &hourly_factor_contract,
        )
        .expect("activity-compressed schedule should resolve");

        assert_eq!(derived.schedule.interval_deltas_hours[..24], vec![1.0; 24]);
        assert_eq!(derived.schedule.interval_deltas_hours[24..], vec![2.0, 2.0]);
    }

    #[test]
    fn activity_compressed_schedule_collapses_closed_window_between_transitions() {
        let hourly_factor_contract = build_open_closed_hourly_factor_contract(
            utc_nanos(2026, 2, 6, 22, 0, 0),
            &[
                true, false, false, false, false, false, false, false, false, false, false, false,
                false, false, false, false, false, false, false, false, false, false, false, false,
                false, false, false, true, true,
            ],
        );

        let derived = derive_activity_compressed_pricing_interval_schedule(
            utc_nanos(2026, 2, 6, 22, 30, 0),
            &[utc_nanos(2026, 2, 8, 3, 0, 0)],
            &hourly_factor_contract,
        )
        .expect("activity-compressed schedule should resolve");

        assert_eq!(
            derived.schedule.interval_end_timestamps_utc_nanos,
            vec![
                utc_nanos(2026, 2, 6, 23, 0, 0),
                utc_nanos(2026, 2, 8, 1, 0, 0),
                utc_nanos(2026, 2, 8, 2, 0, 0),
                utc_nanos(2026, 2, 8, 3, 0, 0),
            ]
        );
    }

    #[test]
    fn activity_compressed_schedule_preserves_expiries_and_reduces_interval_count() {
        let hourly_factor_contract =
            build_open_closed_hourly_factor_contract(utc_nanos(2026, 2, 2, 14, 0, 0), &[true; 32]);

        let compressed = derive_activity_compressed_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 0, 0),
            &[
                utc_nanos(2026, 2, 2, 20, 0, 0),
                utc_nanos(2026, 2, 3, 22, 0, 0),
            ],
            &hourly_factor_contract,
        )
        .expect("activity-compressed schedule should resolve");
        let exact = derive_pricing_interval_schedule(
            utc_nanos(2026, 2, 2, 14, 0, 0),
            &[
                utc_nanos(2026, 2, 2, 20, 0, 0),
                utc_nanos(2026, 2, 3, 22, 0, 0),
            ],
            &PricingIntervalSchedulePolicy::AutoExactSparse,
        )
        .expect("exact sparse schedule should resolve");

        assert!(compressed.schedule.interval_count() < exact.schedule.interval_count());
        assert!(
            compressed
                .schedule
                .interval_end_timestamps_utc_nanos
                .contains(&utc_nanos(2026, 2, 2, 20, 0, 0))
        );
        assert!(
            compressed
                .schedule
                .interval_end_timestamps_utc_nanos
                .contains(&utc_nanos(2026, 2, 3, 6, 0, 0))
        );
    }
}
