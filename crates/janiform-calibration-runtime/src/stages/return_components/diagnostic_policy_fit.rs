use super::contracts::ReturnComponentsStageDiagnosticPolicy;

const RETURN_COMPONENTS_STAGE_DIAGNOSTIC_POLICY: &str =
    "shock_evidence_thresholded_component_diagnostics_with_named_evidence_states.v1";
const RETURN_COMPONENTS_STAGE_DIAGNOSTIC_INSUFFICIENT_EVIDENCE_STATE: &str =
    "insufficient_evidence";
const RETURN_COMPONENTS_STAGE_DIAGNOSTIC_IDENTIFIED_COMPONENT_STATE: &str = "identified_component";

pub(crate) fn fit_diagnostic_policy(
    shock_evidence_min_count: usize,
) -> ReturnComponentsStageDiagnosticPolicy {
    ReturnComponentsStageDiagnosticPolicy {
        fit_policy: RETURN_COMPONENTS_STAGE_DIAGNOSTIC_POLICY,
        shock_evidence_min_count,
        insufficient_evidence_state: RETURN_COMPONENTS_STAGE_DIAGNOSTIC_INSUFFICIENT_EVIDENCE_STATE,
        identified_component_state: RETURN_COMPONENTS_STAGE_DIAGNOSTIC_IDENTIFIED_COMPONENT_STATE,
    }
}

pub(crate) fn evidence_state_from_count(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    event_count: usize,
) -> &'static str {
    if event_count < diagnostic_policy.shock_evidence_min_count {
        diagnostic_policy.insufficient_evidence_state
    } else {
        diagnostic_policy.identified_component_state
    }
}

#[cfg(test)]
mod tests {
    use super::{evidence_state_from_count, fit_diagnostic_policy};

    #[test]
    fn evidence_state_uses_policy_threshold_and_labels() {
        let diagnostic_policy = fit_diagnostic_policy(2);

        assert_eq!(
            evidence_state_from_count(&diagnostic_policy, 1),
            "insufficient_evidence"
        );
        assert_eq!(
            evidence_state_from_count(&diagnostic_policy, 2),
            "identified_component"
        );
    }
}
