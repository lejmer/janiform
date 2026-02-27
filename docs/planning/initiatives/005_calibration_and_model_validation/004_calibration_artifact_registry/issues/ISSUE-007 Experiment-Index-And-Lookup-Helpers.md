## ISSUE-007: Experiment Index And Lookup Helpers

### Description
Implement lightweight indexing and lookup helpers so teams can find persisted calibration runs by model version, data snapshot, metric outcomes, and timestamps.

### Acceptance Criteria
- Index schema supports key lookup dimensions used in calibration analysis.
- Lookup API supports deterministic query results and pagination or filtering.
- Index is updated as part of artifact commit flow for persisted runs.
- Index rebuild path is available for recovery scenarios.
- Index behavior and docs align with persistence defaults: artifacts are optional, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled.
- Index behavior is scoped to runs where `execution_mode=disk` and artifacts exist.

### Unit tests
- Index record creation and update behavior and index rebuild from existing artifact store.
- Lookup filtering and deterministic ordering.

### Notes/Constraints
- Keep index lightweight and derived from manifest metadata.
- Avoid introducing external database dependencies in v0.1.
