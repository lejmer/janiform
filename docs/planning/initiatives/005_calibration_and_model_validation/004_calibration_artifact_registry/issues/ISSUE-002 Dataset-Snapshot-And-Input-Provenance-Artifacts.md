## ISSUE-002: Dataset Snapshot And Input Provenance Artifacts

### Description
Implement artifact schema and persistence path for dataset snapshots and input provenance metadata used by calibration runs.

### Acceptance Criteria
- Validation catches missing provenance fields and inconsistent checksums
- Output schema and metadata are machine-readable and documented
- Persistence behavior follows explicit defaults: optional overall, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled

### Unit tests
- Round-trip tests for dataset provenance read and write
- Dataset artifact schema validation and checksum field presence and consistency checks

### Notes/Constraints
- Keep raw data references lightweight to avoid oversized artifacts
- Align provenance naming with existing calibration stage terminology
