## Event Tape Schema And State Snapshot Capture

### Description
Define event tape schema and implement capture of per-event diagnostics fields including timestamp, side, and selected Hawkes state snapshot values.

### Acceptance Criteria
- Optional state snapshot payload captures 4D excitation state at defined points
- Serialization format supports deterministic output ordering
- Output schema and metadata are machine-readable and documented
- Event tape schema stores event-time timestamps directly and does not require a fixed bucket or lattice identifier for event placement
- Snapshot fields needed for later coarse-consistency diagnostics are included or explicitly versioned for extension

### Unit tests
- Snapshot correctness at event boundaries
- Schema tests for required event fields and optional state snapshot fields
- Serialization tests for deterministic output ordering
- Tests confirming tied or near-tied event-time timestamps serialize deterministically without grid snapping assumptions

### Notes/Constraints
- Keep snapshot capture configurable to control runtime overhead
- Avoid introducing exchange-specific fields in the core tape schema
