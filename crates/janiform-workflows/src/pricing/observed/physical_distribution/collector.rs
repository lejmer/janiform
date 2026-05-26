use janiform_terminal_ui::ProgressAdapter;

use crate::error::WorkflowError;
use crate::progress::bounded_progress_measure;

use super::super::super::validation::pricing_validation_error;
use super::super::progress::{ObservedWorkItemUpdate, emit_observed_stage_progress};
use super::sampling::{PhysicalDistributionPartitionOutput, PhysicalDistributionProgress};

pub(super) struct PhysicalDistributionCollector<'adapter, 'progress> {
    snapshot_count: usize,
    partition_outputs: Vec<Option<PhysicalDistributionPartitionOutput>>,
    completed_path_count: usize,
    total_path_count: usize,
    progress_adapter: &'adapter mut Option<&'progress mut ProgressAdapter>,
}

impl<'adapter, 'progress> PhysicalDistributionCollector<'adapter, 'progress> {
    pub(super) fn new(
        snapshot_count: usize,
        partition_count: usize,
        path_count_per_snapshot: usize,
        progress_adapter: &'adapter mut Option<&'progress mut ProgressAdapter>,
    ) -> Self {
        Self {
            snapshot_count,
            partition_outputs: (0..partition_count).map(|_| None).collect(),
            completed_path_count: 0,
            total_path_count: snapshot_count * path_count_per_snapshot,
            progress_adapter,
        }
    }

    pub(super) fn record_progress(
        &mut self,
        progress: PhysicalDistributionProgress,
    ) -> Result<(), WorkflowError> {
        self.completed_path_count += progress.completed_path_count;

        emit_observed_stage_progress(
            self.progress_adapter,
            "physical distribution diagnostics",
            bounded_progress_measure(self.completed_path_count, self.total_path_count)?,
            Some(format!(
                "simulated {}/{} P paths",
                self.completed_path_count, self.total_path_count
            )),
            ObservedWorkItemUpdate::none(),
        )
    }

    pub(super) fn record_completed(
        &mut self,
        job_position: usize,
        output: PhysicalDistributionPartitionOutput,
    ) -> Result<(), WorkflowError> {
        let slot = self
            .partition_outputs
            .get_mut(job_position)
            .ok_or_else(|| {
                pricing_validation_error(
                    "physical_distribution_diagnostics",
                    "worker returned an unknown path partition",
                )
            })?;

        *slot = Some(output);

        Ok(())
    }

    pub(super) fn finish(self) -> Result<Vec<Vec<Vec<f64>>>, WorkflowError> {
        let mut terminal_returns_by_snapshot = vec![Vec::<Vec<f64>>::new(); self.snapshot_count];

        for output in self.partition_outputs.into_iter() {
            let output = output.ok_or_else(|| {
                pricing_validation_error(
                    "physical_distribution_diagnostics",
                    "missing worker output for path partition",
                )
            })?;

            if let Some(error) = output.error {
                return Err(WorkflowError::PricingExecutionFailed { message: error });
            }

            let snapshot_returns = terminal_returns_by_snapshot
                .get_mut(output.snapshot_position)
                .ok_or_else(|| {
                    pricing_validation_error(
                        "physical_distribution_diagnostics",
                        "worker returned an unknown valuation state",
                    )
                })?;

            if snapshot_returns.is_empty() {
                snapshot_returns.resize_with(output.group_terminal_returns.len(), Vec::new);
            }

            for (group_position, group_returns) in
                output.group_terminal_returns.into_iter().enumerate()
            {
                let snapshot_group_returns =
                    snapshot_returns.get_mut(group_position).ok_or_else(|| {
                        pricing_validation_error(
                            "physical_distribution_diagnostics",
                            "worker returned an unknown maturity group",
                        )
                    })?;
                snapshot_group_returns.extend(group_returns);
            }
        }

        Ok(terminal_returns_by_snapshot)
    }
}
