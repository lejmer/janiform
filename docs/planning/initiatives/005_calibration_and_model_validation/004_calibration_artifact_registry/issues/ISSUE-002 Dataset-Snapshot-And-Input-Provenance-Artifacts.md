## ISSUE-002: Dataset Snapshot And Input Provenance Artifacts

### Description
Implement artifact schema and persistence path for dataset snapshots and input provenance metadata used by calibration runs.

### Acceptance Criteria
- Dataset artifact stores snapshot identifiers and source metadata.
- Provenance fields include data window, preprocessing config, and checksum metadata.
- Artifact format is machine-readable and linked from manifest.
- Validation catches missing provenance fields and inconsistent checksums.

### Unit tests
- Tests for dataset artifact schema validation.
- Tests for checksum field presence and consistency checks.
- Round-trip tests for dataset provenance read and write.

### Notes/Constraints
- Keep raw data references lightweight to avoid oversized artifacts.
- Align provenance naming with existing calibration stage terminology.
