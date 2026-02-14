## ISSUE-007: Failure Diagnostics And Triage Metadata Outputs

### Description
Build structured diagnostics output for invariant and regression failures so root-cause isolation is fast across constraints, dynamics, and random sampling paths.

### Acceptance Criteria
- Test failures emit structured metadata including invariant ID, scenario ID, seed, and parameter snapshot.
- Diagnostics classify failures by category such as constraint, transition, ordering, or statistical drift.
- Metadata format is documented and consumable by CI and local tooling.
- At least one triage report example is included in project documentation.

### Unit tests
- Tests for diagnostics payload schema and required fields.
- Tests ensuring failure category classification is deterministic.
- Integration tests confirming metadata emission from both deterministic and statistical suites.

### Notes/Constraints
- Keep diagnostics concise but sufficient for immediate root-cause triage.
- Avoid embedding environment-specific paths in emitted metadata.
