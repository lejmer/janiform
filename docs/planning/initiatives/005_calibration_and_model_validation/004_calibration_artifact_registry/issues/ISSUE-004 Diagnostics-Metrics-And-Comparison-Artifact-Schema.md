## ISSUE-004: Diagnostics Metrics And Comparison Artifact Schema

### Description
Implement schema for diagnostics, metric outputs, and comparison results so calibration quality can be reviewed and replayed.

### Acceptance Criteria
- Artifact schema supports stylized-fact metrics and scale-consistency comparison outputs.
- Metric records include horizon, threshold, and pass or fail metadata.
- Artifact links to run manifest and stage outputs.
- Validation catches incomplete metric records and invalid references.

### Unit tests
- Tests for diagnostics artifact schema validation.
- Tests for required threshold and status metadata.
- Tests for artifact-reference integrity to manifest and stage outputs.

### Notes/Constraints
- Reuse metric naming conventions from diagnostics projects.
- Keep artifact schema stable for downstream reporting tools.
