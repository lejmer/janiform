## Performance Baseline Capture And Versioned Storage

### Description
Implement storage and versioning for performance baselines so latency and throughput changes can be tracked over time.

### Acceptance Criteria
- Baselines are versioned and reloadable for comparison runs
- Documentation defines baseline refresh process and ownership
- Output schema and metadata are machine-readable and documented

### Unit tests
- Baseline read and write round-trip integrity
- Baseline artifact schema validation
- Comparison logic across baseline versions

### Notes/Constraints
- Keep artifact payloads concise but sufficient for triage
- Avoid environment-specific unstable fields where possible
