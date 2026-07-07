use super::direct::DirectSimulationJob;
use super::request::ModelArtifactSimulationKind;

/// Runtime simulation flavor selected by thin direct runtime wrappers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeSimulationKind {
    /// Coarse interval runtime simulation.
    CoarseInterval,
    /// Seeded tick-event runtime simulation.
    SeededTick,
}

impl RuntimeSimulationKind {
    /// Human-readable runtime scope label.
    pub(super) const fn scope_label(self) -> &'static str {
        match self {
            Self::CoarseInterval => "Simulating coarse intervals",
            Self::SeededTick => "Simulating seeded ticks",
        }
    }

    /// User-readable expected output label.
    pub(super) const fn expected_output_label(self) -> &'static str {
        match self {
            Self::CoarseInterval => "coarse interval simulation",
            Self::SeededTick => "seeded tick-event simulation",
        }
    }
}

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
