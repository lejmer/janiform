## Event Tape Schema And State Snapshot Capture

### Description
Define event tape schema and implement capture of per-event diagnostics fields including timestamp, side, and selected Hawkes state snapshot values.

### Acceptance Criteria
- Optional state snapshot payload captures 4D excitation state at defined points
- Serialization format supports deterministic output ordering
- Output schema and metadata are machine-readable and documented

### Unit tests
- Snapshot correctness at event boundaries
- Schema tests for required event fields and optional state snapshot fields
- Serialization tests for deterministic output ordering

### Notes/Constraints
- Keep snapshot capture configurable to control runtime overhead
- Avoid introducing exchange-specific fields in the core tape schema
