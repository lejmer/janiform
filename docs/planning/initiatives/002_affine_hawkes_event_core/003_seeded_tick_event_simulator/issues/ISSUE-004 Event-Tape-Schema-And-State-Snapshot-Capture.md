## ISSUE-004: Event Tape Schema And State Snapshot Capture

### Description
Define event tape schema and implement capture of per-event diagnostics fields including timestamp, side, and selected Hawkes state snapshot values.

### Acceptance Criteria
- Event tape schema includes timestamp, side, and stable event identifier fields.
- Optional state snapshot payload captures 4D excitation state at defined points.
- Tape schema is consumable by diagnostics and calibration workflows.
- Serialization format supports deterministic output ordering.

### Unit tests
- Schema tests for required event fields and optional state snapshot fields.
- Tests for snapshot correctness at event boundaries.
- Serialization tests for deterministic output ordering.

### Notes/Constraints
- Keep snapshot capture configurable to control runtime overhead.
- Avoid introducing exchange-specific fields in the core tape schema.
