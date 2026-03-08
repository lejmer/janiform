## Session Schedule Loader And Immutable Registry

### Description
Implement loading of exchange calendar artifacts into an immutable in-memory registry that can be queried by exchange identifier.

### Acceptance Criteria
- A loader can read calendar artifacts and build registry entries for one or more exchanges.
- Loaded registry objects are immutable after construction.
- Loader returns structured errors for unknown fields, malformed dates, and duplicate exchange IDs.
- Registry lookup API supports deterministic behavior for missing exchange IDs.

### Unit tests
- Loader tests for valid artifact ingestion.
- Negative tests for malformed artifacts and duplicate IDs.
- Registry lookup tests for existing and missing exchanges.

### Notes/Constraints
- Artifact format should be versioned to support future schema evolution.
- Keep parsing logic separate from query logic.
