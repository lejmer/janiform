use std::sync::Arc;

use janiform::simulation::{
    CoarseSimulationRunError, CoarseSimulationRunRequest, run_coarse_simulation,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::randomness::{
    SimulationRandomnessSubstreamDomain, derive_simulation_substream_seed,
};
use janiform_foundations::time::checked_ceil_interval_count_between_utc_nanos;
use janiform_work_scheduler::{StreamingWorkContext, balanced_work_ranges};

use crate::error::WorkflowError;

use super::super::super::validation::pricing_validation_error;
use super::super::types::DEFAULT_PRICING_INTERVAL_DURATION_NANOS;
use super::types::ObservedOptionsPhysicalDistributionDiagnosticsRequest;

pub(super) struct PreparedPhysicalDistributionSnapshot {
    pub(super) simulation_request_template: CoarseSimulationRunRequest,
    pub(super) terminal_record_indices: Vec<usize>,
    pub(super) initial_observed_log_price: f64,
    pub(super) horizon_interval_count: usize,
}

pub(super) struct PhysicalDistributionJob {
    prepared_snapshot: Arc<PreparedPhysicalDistributionSnapshot>,
    diagnostics_request: ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    snapshot_position: usize,
    path_start: usize,
    path_end: usize,
}

pub(super) struct PhysicalDistributionProgress {
    pub(super) completed_path_count: usize,
}

pub(super) struct PhysicalDistributionPartitionOutput {
    pub(super) snapshot_position: usize,
    pub(super) group_terminal_returns: Vec<Vec<f64>>,
    pub(super) error: Option<String>,
}

pub(super) fn build_physical_distribution_jobs(
    prepared: &[Arc<PreparedPhysicalDistributionSnapshot>],
    request: ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    worker_count: usize,
) -> Vec<PhysicalDistributionJob> {
    let partition_count_per_valuation = worker_count.min(request.path_count).max(1);

    let mut jobs = Vec::with_capacity(prepared.len() * partition_count_per_valuation);

    for (snapshot_position, prepared_snapshot) in prepared.iter().enumerate() {
        for path_range in balanced_work_ranges(request.path_count, partition_count_per_valuation) {
            jobs.push(PhysicalDistributionJob {
                prepared_snapshot: Arc::clone(prepared_snapshot),
                diagnostics_request: request,
                snapshot_position,
                path_start: path_range.start,
                path_end: path_range.end,
            });
        }
    }

    jobs
}

pub(super) fn execute_physical_distribution_job(
    job: PhysicalDistributionJob,
    context: StreamingWorkContext<
        PhysicalDistributionProgress,
        Result<PhysicalDistributionPartitionOutput, WorkflowError>,
    >,
) -> Result<PhysicalDistributionPartitionOutput, WorkflowError> {
    let cancellation_token = context.cancellation_token();

    let mut group_terminal_returns = vec![
        Vec::with_capacity(job.path_end - job.path_start);
        job.prepared_snapshot.terminal_record_indices.len()
    ];

    for path_position in job.path_start..job.path_end {
        cancellation_token.check_cancelled()?;

        let path_result =
            simulate_terminal_returns(&job, path_position, cancellation_token.shared_handle());
        match path_result {
            Ok(terminal_returns) => {
                for (group_position, terminal_return) in terminal_returns.into_iter().enumerate() {
                    if let Some(group_returns) = group_terminal_returns.get_mut(group_position) {
                        group_returns.push(terminal_return);
                    }
                }
            }
            Err(error) => {
                return Ok(PhysicalDistributionPartitionOutput {
                    snapshot_position: job.snapshot_position,
                    group_terminal_returns,
                    error: Some(error.to_string()),
                });
            }
        }
    }

    cancellation_token.check_cancelled()?;

    if !context.send_progress(PhysicalDistributionProgress {
        completed_path_count: job.path_end - job.path_start,
    }) {
        cancellation_token.check_cancelled()?;
    }

    Ok(PhysicalDistributionPartitionOutput {
        snapshot_position: job.snapshot_position,
        group_terminal_returns,
        error: None,
    })
}

pub(super) fn terminal_record_index_for_expiry(
    valuation_time_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
) -> Result<usize, WorkflowError> {
    if expiry_timestamp_utc_nanos <= valuation_time_utc_nanos {
        return Err(pricing_validation_error(
            "expiry_timestamp_utc",
            "must be after valuation time",
        ));
    }

    let interval_count = checked_ceil_interval_count_between_utc_nanos(
        valuation_time_utc_nanos,
        expiry_timestamp_utc_nanos,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )
    .ok_or_else(|| {
        pricing_validation_error(
            "expiry_timestamp_utc",
            "terminal interval count must fit in usize",
        )
    })?;

    interval_count.checked_sub(1).ok_or_else(|| {
        pricing_validation_error("expiry_timestamp_utc", "terminal interval must be positive")
    })
}

fn simulate_terminal_returns(
    job: &PhysicalDistributionJob,
    path_position: usize,
    cancellation_token: CancellationToken,
) -> Result<Vec<f64>, WorkflowError> {
    let path_count = u64::try_from(job.diagnostics_request.path_count).map_err(|_| {
        pricing_validation_error(
            "physical_distribution_diagnostics.path_count",
            "must fit in u64",
        )
    })?;

    let snapshot_position = u64::try_from(job.snapshot_position).map_err(|_| {
        pricing_validation_error(
            "physical_distribution_diagnostics",
            "too many valuation states",
        )
    })?;

    let path_position = u64::try_from(path_position).map_err(|_| {
        pricing_validation_error(
            "physical_distribution_diagnostics",
            "path index is too large",
        )
    })?;

    let substream_index = snapshot_position
        .checked_mul(path_count)
        .and_then(|base| base.checked_add(path_position))
        .ok_or_else(|| {
            pricing_validation_error(
                "physical_distribution_diagnostics",
                "path substream index overflowed",
            )
        })?;

    let path_seed = derive_simulation_substream_seed(
        job.diagnostics_request.seed,
        SimulationRandomnessSubstreamDomain::ObservedOptionsPhysicalDistributionPath,
        substream_index,
    );

    // Intentional copy because the coarse simulation runtime owns its request
    // and each diagnostic path needs a distinct deterministic seed.
    let mut simulation_request = job.prepared_snapshot.simulation_request_template.clone();
    simulation_request.seed = path_seed;

    let path = run_coarse_simulation(simulation_request, cancellation_token)
        .map_err(physical_distribution_simulation_error)?;

    job.prepared_snapshot
        .terminal_record_indices
        .iter()
        .map(|&terminal_record_index| {
            let terminal_record = path.records.get(terminal_record_index).ok_or_else(|| {
                WorkflowError::PricingExecutionFailed {
                    message: format!(
                        "physical distribution path ended before terminal interval {terminal_record_index}"
                    ),
                }
            })?;

            Ok(
                terminal_record.observed_log_price_end
                    - job.prepared_snapshot.initial_observed_log_price,
            )
        })
        .collect()
}

fn physical_distribution_simulation_error(source: CoarseSimulationRunError) -> WorkflowError {
    match source {
        CoarseSimulationRunError::Cancelled { source } => WorkflowError::Cancelled { source },
        source => WorkflowError::PricingExecutionFailed {
            message: format!("physical distribution coarse simulation failed: {source}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::terminal_record_index_for_expiry;

    #[test]
    fn terminal_record_index_for_expiry_ceil_covers_partial_terminal_interval() {
        assert_eq!(
            terminal_record_index_for_expiry(0, NANOS_PER_HOUR)
                .expect("exact one-hour maturity should use the first terminal record"),
            0
        );
        assert_eq!(
            terminal_record_index_for_expiry(0, NANOS_PER_HOUR + 1)
                .expect("partial second hour should use the second terminal record"),
            1
        );
    }

    #[test]
    fn terminal_record_index_for_expiry_rejects_non_positive_maturity() {
        assert!(terminal_record_index_for_expiry(NANOS_PER_HOUR, NANOS_PER_HOUR).is_err());
    }
}
