## ISSUE-005: Performance Baseline Capture And Versioned Storage

### Description
Implement storage and versioning for performance baselines so latency and throughput changes can be tracked over time.

### Acceptance Criteria
- Baseline artifacts include scenario ID, component metrics, environment metadata, and commit reference.
- Baselines are versioned and reloadable for comparison runs.
- Storage format supports machine-readable diffing across runs.
- Documentation defines baseline refresh process and ownership.

### Unit tests
- Tests for baseline artifact schema validation.
- Tests for baseline read and write round-trip integrity.
- Tests for comparison logic across baseline versions.

### Notes/Constraints
- Keep artifact payloads concise but sufficient for triage.
- Avoid environment-specific unstable fields where possible.
