use super::progress::StylizedFactsValidationProgressPhase;

/// Human-readable scope label for the stylized-facts validation workflow.
pub const fn stylized_facts_validation_scope_label() -> &'static str {
    "Validate stylized facts"
}

/// Render a stylized-facts validation progress phase for terminal output.
pub const fn stylized_facts_validation_phase_label(
    phase: StylizedFactsValidationProgressPhase,
) -> &'static str {
    match phase {
        StylizedFactsValidationProgressPhase::LoadEmpiricalData => "Load empirical data",
        StylizedFactsValidationProgressPhase::DeriveIntervalReturns => "Derive interval returns",
        StylizedFactsValidationProgressPhase::SimulateSeededPath => "Simulate seeded paths",
        StylizedFactsValidationProgressPhase::ComputeMetrics => "Compute return-based metrics",
        StylizedFactsValidationProgressPhase::CompareMetrics => "Compare metrics",
        StylizedFactsValidationProgressPhase::WriteArtifacts => "Write artifacts",
    }
}

/// Terminal progress stages for the stylized-facts validation workflow.
pub const STYLIZED_FACTS_VALIDATION_STAGE_LABELS: &[&str] = &[
    stylized_facts_validation_phase_label(StylizedFactsValidationProgressPhase::LoadEmpiricalData),
    stylized_facts_validation_phase_label(
        StylizedFactsValidationProgressPhase::DeriveIntervalReturns,
    ),
    stylized_facts_validation_phase_label(StylizedFactsValidationProgressPhase::SimulateSeededPath),
    stylized_facts_validation_phase_label(StylizedFactsValidationProgressPhase::ComputeMetrics),
    stylized_facts_validation_phase_label(StylizedFactsValidationProgressPhase::CompareMetrics),
    stylized_facts_validation_phase_label(StylizedFactsValidationProgressPhase::WriteArtifacts),
];
