## ISSUE-004: Diagnostics Metrics And Comparison Artifact Schema

### Description
Implement schema for persisted diagnostics, metric outputs, and comparison results so calibration quality can be reviewed and replayed for persisted runs.

### Acceptance Criteria
- Artifact links to run manifest and stage outputs.
- Validation catches incomplete metric records and invalid references.
- Output schema and metadata are machine-readable and documented.
- Persistence behavior follows explicit defaults: optional overall, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled.
- Artifact requirements are scoped to `execution_mode=disk` runs where persistence is enabled.

### Unit tests
- Required threshold and status metadata and artifact-reference integrity to manifest and stage outputs.
- Diagnostics artifact schema validation.

### Notes/Constraints
- Reuse metric naming conventions from diagnostics projects.
- Keep artifact schema stable for downstream reporting tools.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
