## ISSUE-001: Artifact Manifest Schema And Versioning Policy

### Description
Define the top-level artifact manifest schema and versioning policy for calibration runs, including compatibility rules for v0.x.

### Acceptance Criteria
- A canonical manifest schema exists with run ID, artifact references, and schema version.
- Versioning policy defines compatibility guarantees for v0.x artifacts.
- Manifest validation rejects missing required fields and invalid versions.
- Policy documentation includes upgrade and deprecation guidance.

### Unit tests
- Tests for manifest schema validation on valid and invalid payloads.
- Tests for version compatibility checks and expected failure cases.
- Tests for required-field enforcement.

### Notes/Constraints
- Keep manifest stable and minimal to reduce migration overhead.
- Avoid embedding large payloads directly in the manifest.
