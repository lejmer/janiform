use std::sync::Arc;

use crate::stages::joint_refinement::contracts::JointRefinementStageExecutionFailure;
use crate::stages::parallelism::resolved_stage_cpu_count;

use super::super::super::summary_simulation::JointRefinementSummarySimulationSessionContext;
use super::super::super::worker_runtime::JointRefinementSummarySimulationScheduler;
use super::super::{JointRefinementContext, simulation_infrastructure_failure_message};

pub(super) fn ensure_simulation_infrastructure_available(
    bootstrap_context: &JointRefinementContext,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if let Some(error_message) = simulation_infrastructure_failure_message(bootstrap_context) {
        return Err(
            JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                error_message,
            },
        );
    }

    Ok(())
}

pub(super) fn build_summary_simulation_scheduler(
    cpu_count: usize,
    summary_simulation_session_context: &JointRefinementSummarySimulationSessionContext,
) -> Result<Arc<JointRefinementSummarySimulationScheduler>, JointRefinementStageExecutionFailure> {
    JointRefinementSummarySimulationScheduler::new(
        resolved_stage_cpu_count(cpu_count),
        summary_simulation_session_context,
    )
    .map(Arc::new)
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable { error_message }
    })
}
