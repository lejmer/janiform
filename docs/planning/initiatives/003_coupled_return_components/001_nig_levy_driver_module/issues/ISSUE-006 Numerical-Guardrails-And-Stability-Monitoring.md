## ISSUE-006: Numerical Guardrails And Stability Monitoring

### Description
Add numerical guardrails for near-boundary NIG parameter regimes and transform arguments, with diagnostics that help identify unstable evaluations.

### Acceptance Criteria
- Guardrails detect near-domain-violation inputs and report structured warnings or errors.
- Evaluation path includes stable handling policies for sensitive numeric regions.
- Diagnostics expose enough metadata to triage instability in calibration and pricing runs.
- Guardrail policy is documented and configurable where appropriate.

### Unit tests
- Tests for guardrail triggers near `alpha` and `beta` boundary conditions.
- Tests for stable behavior under representative extreme argument ranges.
- Tests for diagnostic payload schema and required fields.

### Notes/Constraints
- Keep guardrail behavior deterministic.
- Do not silently clip invalid inputs without reporting.
