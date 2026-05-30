use super::direct::DirectSimulationJob;
use super::request::ModelArtifactSimulationKind;

/// Persisted workflow label for simulation artifacts and command output.
pub const fn simulation_workflow_label() -> &'static str {
    "simulate"
}

/// Resolve the human-readable scope label for a direct simulation job.
pub fn direct_simulation_scope_label(job: &DirectSimulationJob) -> &'static str {
    match job {
        DirectSimulationJob::SeededTick { .. } => "Seeded tick simulation",
        DirectSimulationJob::CoarseInterval { .. } => "Coarse interval simulation",
    }
}

/// Resolve the human-readable scope label for a model-artifact simulation workflow.
pub const fn model_artifact_simulation_scope_label(
    simulation_kind: ModelArtifactSimulationKind,
) -> &'static str {
    match simulation_kind {
        ModelArtifactSimulationKind::SeededTick => "Model artifact seeded tick simulation",
        ModelArtifactSimulationKind::CoarseInterval => "Model artifact coarse interval simulation",
    }
}

/// Human-readable scope label for direct coarse runtime simulation.
pub(super) const fn coarse_runtime_simulation_scope_label() -> &'static str {
    "Simulating coarse intervals"
}

/// Human-readable scope label for direct seeded tick runtime simulation.
pub(super) const fn seeded_tick_runtime_simulation_scope_label() -> &'static str {
    "Simulating seeded ticks"
}
