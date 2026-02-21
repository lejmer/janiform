## Failure Diagnostics And Triage Metadata Outputs

### Description
Build structured diagnostics output for invariant and regression failures so root-cause isolation is fast across constraints, dynamics, and random sampling paths.

### Acceptance Criteria
- Diagnostics classify failures by category such as constraint, transition, ordering, or statistical drift
- At least one triage report example is included in project documentation
- Output schema and metadata are machine-readable and documented
- Diagnostics include dedicated categories for scale-consistency drift and event-time grid-leakage or timestamp-snapping regressions

### Unit tests
- Diagnostics payload schema and required fields
- Tests ensuring failure category classification is deterministic
- Integration tests confirming metadata emission from both deterministic and statistical suites
- Integration tests for diagnostics emitted by coarse-vs-aggregated mismatch scenarios

### Notes/Constraints
- Keep diagnostics concise but sufficient for immediate root-cause triage
- Avoid embedding environment-specific paths in emitted metadata
