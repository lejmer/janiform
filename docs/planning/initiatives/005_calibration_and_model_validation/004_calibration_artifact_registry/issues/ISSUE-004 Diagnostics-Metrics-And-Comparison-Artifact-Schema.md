## ISSUE-004: Diagnostics Metrics And Comparison Artifact Schema

### Description
Implement schema for diagnostics, metric outputs, and comparison results so calibration quality can be reviewed and replayed.

### Acceptance Criteria
- Artifact links to run manifest and stage outputs
- Validation catches incomplete metric records and invalid references
- Output schema and metadata are machine-readable and documented

### Unit tests
- Required threshold and status metadata and artifact-reference integrity to manifest and stage outputs
- Diagnostics artifact schema validation

### Notes/Constraints
- Reuse metric naming conventions from diagnostics projects
- Keep artifact schema stable for downstream reporting tools
